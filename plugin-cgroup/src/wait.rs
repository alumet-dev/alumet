//! Tools to wait for the cgroupfs to be mounted.
//!
//! This module is currently implemented on top of a filesystem notification mechanism,
//! but this could change in the future (e.g. to rely on libudev instead).

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use anyhow::Context;
use notify::{Event, EventHandler, EventKind, RecursiveMode, Watcher};

/// Which mechanism to use to detect the mount.
pub enum MountWaitMechanism {
    /// Use the recommended notify watcher for the current platform.
    NotifyAuto,
    /// Use [`notify::PollWatcher`], which repeatedly polls the filesystem to detect changes.
    NotifyPoll { interval: Duration },
}

/// A `MountWait` represents an active wait for a cgroupfs to be mounted on some path.
/// 
/// Dropping the `MounWait` will stop the watch.
pub struct MountWait {
    // We need to keep the watcher alive, because it stops on drop.
    #[allow(dead_code)]
    watcher: Box<dyn Watcher>,
}

impl MountWaitMechanism {
    fn new_watcher(self, handler: impl EventHandler) -> notify::Result<Box<dyn Watcher>> {
        match self {
            MountWaitMechanism::NotifyAuto => Ok(Box::new(notify::recommended_watcher(handler)?)),
            MountWaitMechanism::NotifyPoll { interval } => {
                let config = notify::Config::default().with_poll_interval(interval);
                Ok(Box::new(notify::PollWatcher::new(handler, config)?))
            }
        }
    }
}

impl MountWait {
    /// Starts to watch for a mount of the cgroupfs at the given `path`.
    pub fn new<F: FnMut(anyhow::Result<PathBuf>) + Send + 'static>(
        path: &Path,
        on_mount_or_error: F,
        mechanism: MountWaitMechanism,
    ) -> anyhow::Result<Self> {
        let handler = Handler::new(path, on_mount_or_error);
        let mut watcher = mechanism.new_watcher(handler).context("watcher init failed")?;
        watcher
            .watch(path, RecursiveMode::NonRecursive)
            .context("watcher.watch failed")?;
        Ok(Self { watcher })
    }
}

/// Checks whether a dir is empty.
///
/// Returns an error if it is impossible to check whether the dir is empty,
/// for instance if it does not exist, or if the user does not have the
/// necessary permissions.
fn is_dir_empty(dir: &Path) -> std::io::Result<bool> {
    Ok(std::fs::read_dir(dir)?.next().is_none())
}

struct Handler<F: FnMut(anyhow::Result<PathBuf>) + Send + 'static> {
    wanted_path: PathBuf,
    callback: Option<F>,
    watcher: Option<Weak<Mutex<dyn Watcher + Send + Sync + 'static>>>,
    run_fallback_loop: Arc<AtomicBool>,
}

const POPULATE_SMALL_WAIT: Duration = Duration::from_millis(50);
const POPULATE_LOOP_WAIT: Duration = Duration::from_secs(2);

impl<F: FnMut(anyhow::Result<PathBuf>) + Send + 'static> Handler<F> {
    fn new(wanted_path: &Path, callback: F) -> Self {
        Self {
            wanted_path: wanted_path.to_owned(),
            callback: Some(callback),
            watcher: None,
            run_fallback_loop: Arc::new(AtomicBool::new(true)),
        }
    }

    fn propagate_error(&mut self, err: impl Into<anyhow::Error>) {
        if let Some(f) = &mut self.callback {
            f(Err(err.into()))
        }
    }

    fn run_callback(&mut self) {
        // Take the callback to avoid calling it multiple times with Ok.
        if let Some(mut f) = self.callback.take() {
            f(Ok(self.wanted_path.clone()));
        }
    }

    fn on_event(&mut self, event: notify::Result<Event>) -> anyhow::Result<()> {
        let event = event?;
        if matches!(event.kind, EventKind::Create(_)) && event.paths.contains(&self.wanted_path) {
            // The directory that we want now exists.
            // BUT mounting the cgroupfs is done in two steps:
            // - create the directory -> this is detected by notify
            // - mount cgroupfs to it
            //
            // To check that the cgroupfs has been mounted, we check whether the directory is empty or not.
            // This is not a perfect check, but it works for our use case.

            if is_dir_empty(&self.wanted_path)? {
                // wait a little bit, if mkdir is immediately followed by mount, it should not take long
                std::thread::sleep(POPULATE_SMALL_WAIT);

                if is_dir_empty(&self.wanted_path)? {
                    // Okay, it's still empty, we may have to wait longer. Let's setup up a proper watch.
                    let watcher = self
                        .watcher
                        .as_mut()
                        .expect("watcher should be set on the handler before the first watch is added");

                    // The watcher may be gone, which means that its owner has dropped it, and we should stop watching.
                    if let Some(watcher) = watcher.upgrade() {
                        // BEWARE: calling watcher.watch from inside the EventHandler can deadlock on some watchers,
                        // in particular with PollWatcher. See https://github.com/notify-rs/notify/issues/463
                        // Workaround: use another thread, so that the watch is added when the handler has finished.
                        let path_to_watch = self.wanted_path.clone();
                        let run_fallback_loop = self.run_fallback_loop.clone();
                        if let Some(mut callback) = self.callback.take() {
                            std::thread::spawn(move || {
                                let mut watcher = watcher.lock().unwrap();
                                if let Err(e) = watcher.watch(&path_to_watch, RecursiveMode::NonRecursive) {
                                    log::error!("failed to add watch on {path_to_watch:?}: {e:?}");
                                    // alternative solution: a loop
                                    while run_fallback_loop.load(Ordering::Relaxed) {
                                        match is_dir_empty(&path_to_watch) {
                                            Ok(true) => continue,
                                            Ok(false) => {
                                                callback(Ok(path_to_watch));
                                                return;
                                            }
                                            Err(e) => callback(Err(e.into())),
                                        };
                                        std::thread::sleep(POPULATE_LOOP_WAIT);
                                    }
                                }

                                // While we were setting up the watcher, the directory may have been populated!
                                match is_dir_empty(&path_to_watch) {
                                    Ok(true) => {
                                        // not yet, let the watcher (or fallback loop) do its work
                                    }
                                    Ok(false) => {
                                        // not empty => populated!
                                        callback(Ok(path_to_watch));
                                    }
                                    Err(e) => callback(Err(e.into())),
                                }
                            });
                        }
                    }
                }
            }

            // not empty, run the callback and stop here
            self.run_callback();
            return Ok(());
        }

        let is_dir_populated = event.paths.iter().any(|p| p.parent() == Some(&self.wanted_path));
        if !matches!(event.kind, EventKind::Remove(_)) && is_dir_populated {
            // wanted_path has subdirectories/subfiles, assume that the cgroupfs was mounted
            self.run_callback();
        }
        Ok(())
    }
}

impl<F: FnMut(anyhow::Result<PathBuf>) + Send + 'static> EventHandler for Handler<F> {
    fn handle_event(&mut self, event: notify::Result<Event>) {
        if let Err(err) = self.on_event(event) {
            self.propagate_error(err);
        }
    }
}
