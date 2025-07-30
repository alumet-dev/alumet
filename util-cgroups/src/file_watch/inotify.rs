use std::{
    ffi::OsString,
    fs::Metadata,
    io::ErrorKind,
    os::fd::{AsFd, AsRawFd},
    path::PathBuf,
    sync::Arc,
    thread::JoinHandle,
};

use crate::file_watch::{EventHandler, PathKind, Watcher};
use anyhow::Context;
use mio::{unix::SourceFd, Events, Interest, Poll, Token, Waker};
use nix::{
    errno::Errno,
    sys::inotify::{AddWatchFlags, InitFlags, Inotify, InotifyEvent, WatchDescriptor},
};
use rustc_hash::{FxBuildHasher, FxHashMap, FxHashSet};
use walkdir::WalkDir;

pub struct InotifyWatcher {
    thread_handle: Option<JoinHandle<()>>,
    stop_waker: Arc<Waker>,
}

impl InotifyWatcher {
    /// Creates and starts a new watcher based on `inotify`.
    ///
    /// The directories `paths_to_watch` are watched recursively.
    /// When a subfile or subdir is created or removed, the kernel sends a notification to a background thread, which triggers the `event_handler`.
    pub fn new(
        event_handler: impl EventHandler + Send + 'static,
        paths_to_watch: Vec<PathBuf>,
    ) -> anyhow::Result<Self> {
        // prepare inotify and epoll
        let (mut watch, stop_waker) = WatchLoop::new(event_handler)?;

        // register the paths we want to watch
        // It's important to do it recursively, in case the directory is not empty.
        let mut created = Vec::new();
        let mut deleted = Vec::new();
        for path in paths_to_watch {
            match path.metadata().map(|m| m.is_dir()) {
                Ok(true) => watch.watch_recursively(path, &mut created, &mut deleted)?,
                Ok(false) => return Err(anyhow::anyhow!("cannot watch {path:?} because it is not a directory")),
                Err(e) => return Err(e).context(format!("cannot watch {path:?}")),
            }
        }

        // spawn the thread
        let thread_handle = std::thread::spawn(move || {
            if let Err(e) = watch.run() {
                log::error!("error in inotify-based loop: {e:?}");
            }
            log::debug!("inotify-based loop has stopped")
        });

        Ok(Self {
            thread_handle: Some(thread_handle),
            stop_waker,
        })
    }

    pub fn stop_and_join(mut self) -> anyhow::Result<()> {
        if let Some(h) = self.thread_handle.take() {
            self.stop()?;
            h.join()
                .map_err(|e| anyhow::Error::msg(format!("error in background thread: {e:?}")))
        } else {
            Ok(())
        }
    }
}

impl super::Watcher for InotifyWatcher {
    fn stop(&mut self) -> anyhow::Result<()> {
        self.stop_waker.wake().context("failed to wake up the polling thread")?;
        Ok(())
    }
}

impl Drop for InotifyWatcher {
    fn drop(&mut self) {
        if self.thread_handle.is_some() {
            let _ = self.stop_waker.wake();
        }
    }
}

struct WatchedDir {
    path: PathBuf,
    watch_recursively: bool,
}

/// Main watch loop, to run in a dedicated thread.
///
/// # Implementation Details
/// We use inotify to detect changes in the filesystem.
/// It is combined with `epoll` (wrapped by `mio`'s Poll) to gracefully stop the thread without waiting for a notification.
struct WatchLoop<E: EventHandler> {
    inotify: Inotify,
    epoll: Poll,
    watched_dirs: FxHashMap<WatchDescriptor, WatchedDir>,
    watched_paths: FxHashSet<PathBuf>,
    event_handler: E,
}

/// Arbitrary "large" capacity, because we expect numerous cgroups to exist at the same time.
const INITIAL_CAPACITY: usize = 512;
const WATCH_TOKEN: Token = Token(0);
const STOP_TOKEN: Token = Token(1);

// AddWatchFlags ops are not const
fn flags_add_watch() -> AddWatchFlags {
    AddWatchFlags::IN_CREATE | AddWatchFlags::IN_DELETE | AddWatchFlags::IN_DELETE_SELF
}

impl<E: EventHandler> WatchLoop<E> {
    fn new(event_handler: E) -> anyhow::Result<(Self, Arc<Waker>)> {
        // initialize inotify in non-blocking mode
        let inotify =
            Inotify::init(InitFlags::IN_NONBLOCK | InitFlags::IN_CLOEXEC).context("failed to init inotify")?;

        // initialize epoll
        let epoll = Poll::new().context("failed to init epoll")?;

        // create (and register) a waker to wake epoll from another thread
        // NOTE: it seems to work better when the waker is registered first.
        let stop_waker = Arc::new(Waker::new(epoll.registry(), STOP_TOKEN).context("failed to create waker")?);

        // register inotify
        let inotify_fd = inotify.as_fd().as_raw_fd();
        let mut source = SourceFd(&inotify_fd);
        epoll
            .registry()
            .register(&mut source, WATCH_TOKEN, Interest::READABLE)
            .context("failed to registry inotify with epoll")?;

        let s = Self {
            inotify,
            epoll,
            watched_dirs: FxHashMap::with_capacity_and_hasher(INITIAL_CAPACITY, FxBuildHasher),
            watched_paths: FxHashSet::with_capacity_and_hasher(INITIAL_CAPACITY, FxBuildHasher),
            event_handler,
        };
        Ok((s, stop_waker))
    }

    fn watch_recursively(
        &mut self,
        path: PathBuf,
        created: &mut Vec<(PathBuf, PathKind)>,
        deleted: &mut Vec<(PathBuf, PathKind)>,
    ) -> anyhow::Result<()> {
        log::trace!("watch_recursively {path:?}");
        // Iterate on `path` and its sub-directories (the first item yielded by the iterator is `path`).
        // WHY: Doing `add_watch_dir` is not enough, because the directory can be modified (e.g. sub-directories can be created) between the inotify event and the registration of the directory. To avoid missing any event, we need to recursively list the sub-directories.
        for (entry, metadata) in WalkDir::new(&path).into_iter().filter_map(entry_metadata) {
            let path = entry.into_path();
            if metadata.is_dir() {
                match self.add_watch_dir(WatchedDir {
                    path: path.clone(),
                    watch_recursively: true,
                }) {
                    Ok(true) => {
                        log::trace!("(rec) created: Directory {path:?}");
                        created.push((path, PathKind::Directory));
                    }
                    Ok(false) => {
                        log::trace!("(rec) already created: {path:?}");
                    }
                    Err(e) if e == Errno::ENOENT => {
                        // The directory has been removed in the meantime!
                        // Mark it as created *and* deleted.
                        log::trace!("(±) Directory {path:?}");
                        deleted.push((path.clone(), PathKind::Directory));
                        self.unmark_watched(&path);
                    }
                    Err(e) => {
                        log::error!("could not add watch to path {path:?}: {e:#}");
                    }
                }
            } else if metadata.is_file() {
                // just a file, emit "created"
                if self.mark_watched(path.clone()) {
                    log::trace!("(rec) created: File {path:?}");
                    created.push((path, PathKind::File));
                } else {
                    log::trace!("(rec) already created: {path:?}");
                }
            }
        }
        Ok(())
    }

    fn run(mut self) -> anyhow::Result<()> {
        let mut events = Events::with_capacity(INITIAL_CAPACITY);

        'outer: loop {
            log::trace!("polling...");
            let poll_res = self.epoll.poll(&mut events, None);
            if let Err(e) = poll_res {
                if e.kind() == ErrorKind::Interrupted {
                    continue; // retry
                } else {
                    return Err(anyhow::Error::new(e).context("poll error")); // propagate error
                }
            }

            for evt in events.iter() {
                if evt.token() == STOP_TOKEN {
                    break 'outer; // stop
                }

                // else, the event is coming from inotify => check inotify
                match self.inotify.read_events() {
                    Ok(fs_events) => {
                        if let Err(e) = self.process_fs_events(fs_events) {
                            self.event_handler
                                .handle_event(Err(e).context("error while processing events"));
                        }
                    }
                    Err(err) if err == Errno::EAGAIN => {
                        // no events read, go back to poll (not sure if this is supposed to happen, but better be safe)
                    }
                    Err(err) => {
                        let err = std::io::Error::from(err);
                        return Err(err).context("failed to read events from inotify");
                    }
                };
            }
        }
        Ok(())
    }

    fn process_fs_events(&mut self, events: Vec<InotifyEvent>) -> anyhow::Result<()> {
        fn full_name(watched: &WatchedDir, event_name: Option<OsString>) -> PathBuf {
            match event_name {
                Some(filename) => watched.path.join(filename),
                None => watched.path.clone(),
            }
        }

        let mut created = Vec::new();
        let mut deleted = Vec::new();
        let mut need_rescan = false;

        for evt in events {
            log::trace!("event: {evt:?}");
            if evt.mask.contains(AddWatchFlags::IN_Q_OVERFLOW) {
                need_rescan = true;
                continue;
            }

            if evt.mask.contains(AddWatchFlags::IN_CREATE) {
                // A file or directory has been created in a watched directory.
                let watched_dir = self.watched_dirs.get(&evt.wd).unwrap();
                let path = full_name(watched_dir, evt.name);
                let kind = if evt.mask.contains(AddWatchFlags::IN_ISDIR) {
                    PathKind::Directory
                } else {
                    PathKind::File
                };
                log::trace!("(+) {kind:?} {path:?}");
                if kind == PathKind::Directory && watched_dir.watch_recursively {
                    self.watch_recursively(path, &mut created, &mut deleted)?;
                } else {
                    if self.mark_watched(path.clone()) {
                        log::trace!("(file) created: {path:?}");
                        created.push((path, kind));
                    } else {
                        log::trace!("(file) already created: {path:?}");
                    }
                }
            } else if evt.mask.contains(AddWatchFlags::IN_DELETE) {
                // A file or directory has been removed from a watched directory.
                let watched_dir = self.watched_dirs.get(&evt.wd).unwrap();
                let path = full_name(watched_dir, evt.name);
                let kind = if evt.mask.contains(AddWatchFlags::IN_ISDIR) {
                    PathKind::Directory
                } else {
                    PathKind::File
                };
                log::trace!("(-) {kind:?} {path:?}");

                // Avoid putting it twice in the "deleted" list.
                if self.unmark_watched(&path) {
                    deleted.push((path, kind));
                }
            } else if evt.mask.contains(AddWatchFlags::IN_DELETE_SELF) {
                // A directory that we watched has been removed.
                let watched_dir = &self.watched_dirs.remove(&evt.wd).unwrap();
                let path = full_name(watched_dir, evt.name);
                log::trace!("(x) Directory {path:?}");

                // IN_DELETE might arrive before IN_DELETE_SELF.
                // In that case, avoid putting the directory twice in the "deleted" list.
                if self.unmark_watched(&path) {
                    deleted.push((path, PathKind::Directory));
                }
            } else if evt.mask.contains(AddWatchFlags::IN_IGNORED) {
                // The wd should be forgotten in IN_DELETE_SELF
                debug_assert!(!self.watched_dirs.contains_key(&evt.wd));
            }
        }

        // call the event handler
        if !created.is_empty() || !deleted.is_empty() {
            let event = super::Event::Fs { created, deleted };
            self.event_handler.handle_event(Ok(event));
        }
        if need_rescan {
            self.event_handler.handle_event(Ok(super::Event::NeedRescan));
        }
        Ok(())
    }

    fn mark_watched(&mut self, path: PathBuf) -> bool {
        self.watched_paths.insert(path)
    }

    fn unmark_watched(&mut self, path: &PathBuf) -> bool {
        self.watched_paths.remove(path)
    }

    fn add_watch_dir(&mut self, watched: WatchedDir) -> nix::Result<bool> {
        if !self.watched_paths.insert(watched.path.clone()) {
            // prevent duplicates
            return Ok(false);
        }

        let flags = flags_add_watch();
        let wd = self.inotify.add_watch(&watched.path, flags)?;
        self.watched_dirs.insert(wd, watched);
        Ok(true)
    }
}

fn entry_metadata(res: walkdir::Result<walkdir::DirEntry>) -> Option<(walkdir::DirEntry, Metadata)> {
    if let Ok(e) = res {
        if let Ok(metadata) = e.metadata() {
            return Some((e, metadata));
        }
    }
    None
}
