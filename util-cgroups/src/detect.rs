//! Detection of cgroups.

use std::{
    fmt::Debug,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::anyhow;
use notify::{
    event::{CreateKind, RemoveKind},
    Watcher,
};
use rustc_hash::{FxBuildHasher, FxHashSet};
use walkdir::WalkDir;

use super::hierarchy::{Cgroup, CgroupHierarchy, CgroupVersion};

/// Detects the creation of Linux control groups.
///
/// `CgroupDetector` holds a background thread that detects new cgroups.
/// When the detector is dropped, the background thread is stopped.
///
/// # Example
///
/// ```no_run
/// use util_cgroups::{
///     detect::{callback, CgroupDetector, Config},
///     hierarchy::CgroupHierarchy
/// };
///
/// let hierarchy: CgroupHierarchy = todo!();
/// let config = Config::default();
/// let detector = CgroupDetector::new(hierarchy, config, callback(|cgroups| {
///     println!("new cgroups detected: {cgroups:?}");
///     Ok(())
/// }));
/// // TODO store detector somewhere, otherwise it will stop when dropped.
/// ```
pub struct CgroupDetector {
    // keeps the watcher alive
    #[allow(dead_code)]
    watcher: Box<dyn Watcher + Send>,

    state: Arc<Mutex<DetectorState>>,
    hierarchy: CgroupHierarchy,
}

pub struct Config {
    /// Time between each refresh of the filesystem watcher.
    ///
    /// Only applies to cgroup v1 hierarchies (cgroupv2 supports inotify).
    pub v1_refresh_interval: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            v1_refresh_interval: Duration::from_secs(30),
        }
    }
}

/// A callback that is called when new cgroups are detected by a [`CgroupDetector`].
pub trait Callback: Send {
    fn on_new_cgroups(&mut self, cgroups: Vec<Cgroup>) -> anyhow::Result<()>;
}

impl<F: for<'a> FnMut(Vec<Cgroup<'a>>) -> anyhow::Result<()> + Send> Callback for F {
    fn on_new_cgroups(&mut self, cgroups: Vec<Cgroup>) -> anyhow::Result<()> {
        self(cgroups)
    }
}

// The impl above should be enough, but it is not.
// Type inference does not infer for<'a> but a specific lifetime, which doesn't work here.
//
/// Helper to build a callback.
pub fn callback(f: impl for<'all> FnMut(Vec<Cgroup<'all>>) -> anyhow::Result<()> + Send) -> impl Callback {
    f
}

/// Internal state of the detector.
///
/// It is shared between the handler and the detector struct, to allow cgroups to be
/// added by the handler and forgotten by the detector.
struct DetectorState {
    /// A set of known cgroupfs, to avoid detecting the same group multiple times.
    /// Can be modified by the handler and by calling methods on `CgroupDetector`.
    known_cgroups: FxHashSet<String>,

    /// Callback that handles detection events.
    callback: Box<dyn Callback>,
}

// NOTE: if the Box<dyn Callback> is ever replaced with a generic, non-boxed, parameter,
// Clone will have to be implemented manually.
#[derive(Clone)]
struct EventHandler {
    hierarchy: CgroupHierarchy,
    state: Arc<Mutex<DetectorState>>,
}

const INITIAL_CAPACITY: usize = 256;

impl CgroupDetector {
    /// Starts a new cgroup detector for the given group hierarchy.
    ///
    /// The `handler` callback will be called each time new cgroups are created in this hierarchy.
    pub fn new(hierarchy: CgroupHierarchy, config: Config, handler: impl Callback + 'static) -> anyhow::Result<Self> {
        // sanity check: the hierarchy root should exist
        match hierarchy.root().try_exists() {
            Ok(true) => (), // fine
            Ok(false) => {
                return Err(anyhow!(
                    "the hierarchy root should exist: missing directory {}",
                    hierarchy.root().display()
                ));
            }
            Err(e) => {
                return Err(anyhow::Error::new(e).context(format!(
                    "could not check the existence of {} - do I have the permission to access it?",
                    hierarchy.root().display()
                )));
            }
        }

        let state = Arc::new(Mutex::new(DetectorState {
            known_cgroups: FxHashSet::with_capacity_and_hasher(INITIAL_CAPACITY, FxBuildHasher),
            callback: Box::new(handler),
        }));

        let handler = EventHandler {
            hierarchy: hierarchy.clone(),
            state: state.clone(),
        };

        let mut watcher: Box<dyn Watcher + Send> = match hierarchy.version() {
            CgroupVersion::V1 => {
                // inotify is not supported, use polling
                let watcher_config = notify::Config::default();
                watcher_config.with_poll_interval(config.v1_refresh_interval);
                let initial_scan_handler = handler.clone();

                // PollWatcher performs the initial scan on its own.
                let watcher = notify::PollWatcher::with_initial_scan(handler, watcher_config, initial_scan_handler)?;
                Box::new(watcher)
            }
            CgroupVersion::V2 => {
                // inotify is supported
                // First, start the watcher. Then, do the initial scan. This way, we will not miss events.
                let watcher = notify::recommended_watcher(handler.clone())?;

                // We need to manually do the initial scan.
                initial_scan(&hierarchy, handler);

                // all good :)
                Box::new(watcher)
            }
        };
        watcher.watch(hierarchy.root(), notify::RecursiveMode::Recursive)?;

        Ok(Self {
            hierarchy,
            watcher,
            state,
        })
    }

    /// Checks whether a cgroup has been detected by this detector.
    ///
    /// `cgroup_path` is the full path of the cgroup in the sysfs,
    /// for example `/sys/fs/cgroup/user.slice/mygroup`.
    pub fn is_known_by_path(&self, cgroup_path: &Path) -> bool {
        match self.hierarchy.cgroup_path_from_fs(cgroup_path) {
            Some(cgroup) => self.is_known(&cgroup),
            None => false,
        }
    }

    /// Checks whether a cgroup has been detected by this detector.
    ///
    /// `cgroup` is the unique name of the cgroup in the cgroup hierarchy,
    /// for example `/user.slice/mygroup`.
    pub fn is_known(&self, cgroup: &str) -> bool {
        self.state.lock().unwrap().known_cgroups.contains(cgroup)
    }

    /// Forgets a cgroup.
    ///
    /// If a control group with the same path is created in the future,
    /// it will trigger the callback again.
    pub fn forget(&mut self, cgroup: &str) -> bool {
        self.state.lock().unwrap().known_cgroups.remove(cgroup)
    }
}

/// Performs an initial scan of the cgroup hierarchy, and call the `handler` for each cgroup found.
fn initial_scan(hierarchy: &CgroupHierarchy, mut handler: EventHandler) {
    let mut initial_cgroup_paths = Vec::with_capacity(INITIAL_CAPACITY);
    for entry_res in WalkDir::new(hierarchy.root()) {
        match entry_res {
            Ok(entry) => {
                if entry.file_type().is_dir() {
                    initial_cgroup_paths.push(entry.into_path());
                }
            }
            Err(err) => handler.handle_error(err),
        }
    }
    handler.register_cgroups(initial_cgroup_paths);
}

impl EventHandler {
    /// Registers new control groups.
    fn register_cgroups(&mut self, paths: Vec<PathBuf>) {
        // For optimization purposes, we register multiple cgroups at once,
        // so that we only need to lock() once.
        let res = {
            let mut state = self.state.lock().unwrap();
            let mut new_cgroups = Vec::with_capacity(paths.len());
            for path in paths {
                let cgroup = Cgroup::from_fs_path(&self.hierarchy, path);
                if state.known_cgroups.insert(cgroup.canonical_path().to_owned()) {
                    // the set did not contain the value: this is a new cgroup
                    new_cgroups.push(cgroup);
                }
            }
            state.callback.on_new_cgroups(new_cgroups)
        }; // unlock the mutex
        if let Err(err) = res {
            self.handle_error(err);
        }
    }

    /// Removes control groups.
    fn forget_cgroups(&mut self, paths: Vec<PathBuf>) {
        let mut state = self.state.lock().unwrap();
        for path in paths {
            let cgroup = Cgroup::from_fs_path(&self.hierarchy, path);
            if !state.known_cgroups.remove(cgroup.canonical_path()) {
                // the set did not contain the value: weird
                log::warn!("tried to forget cgroup {cgroup} but it was not in the map");
            }
        }
    }

    fn handle_error(&mut self, err: impl Debug) {
        log::error!("error in event handler: {err:?}");
    }
}

impl notify::EventHandler for EventHandler {
    fn handle_event(&mut self, event: notify::Result<notify::Event>) {
        // TODO we get a lot of Access(Open(Any)) and Modify(Metadata(Any)) events, can we ignore them at the inotify level instead of in the match below?
        // -> yes, but we have to use inotify directly, not the notify rust wrapper… -> later
        match event {
            Ok(evt) => match evt.kind {
                // TODO notify returns CreateKind::Any instead of CreateKind::Folder with the PollWatcher…
                notify::EventKind::Create(CreateKind::Folder) => {
                    self.register_cgroups(evt.paths);
                }
                notify::EventKind::Remove(RemoveKind::Folder) => {
                    self.forget_cgroups(evt.paths);
                }
                notify::EventKind::Other if evt.flag() == Some(notify::event::Flag::Rescan) => {
                    // TODO handle rescan
                }
                _ => (),
            },
            Err(err) => {
                self.handle_error(err);
            }
        }
    }
}

impl notify::poll::ScanEventHandler for EventHandler {
    fn handle_event(&mut self, event: notify::poll::ScanEvent) {
        // TODO optimize: collect the paths first and then handle them all at once
        // But this is only used with cgroup v1 so it's fine for now…
        match event {
            Ok(path) => {
                if path.is_dir() {
                    self.register_cgroups(vec![path]);
                }
            }
            Err(err) => {
                self.handle_error(err);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    // Manual cgroup testing.

    // use std::time::Duration;
    // use super::super::{detect::callback, hierarchy::CgroupHierarchy};
    // use super::CgroupDetector;

    // TODO: add automatic test, by:
    // - finding a cgroup that we have the right to modify as the current user
    // - creating new child cgroups in this cgroup
    // - checking that they are detected

    // #[test]
    // fn test_new() -> anyhow::Result<()> {
    //     println!("starting");

    //     let hierarchy = CgroupHierarchy::from_root_path("/sys/fs/cgroup")?;
    //     println!("hierarchy: {hierarchy:?}");

    //     let f = callback(|cgroups| {
    //         println!("new cgroups detected: {cgroups:?}");
    //         Ok(())
    //     });
    //     let _detector = CgroupDetector::new(hierarchy, f)?;
    //     println!("detector ready");

    //     std::thread::sleep(Duration::from_secs(10));
    //     println!("done");
    //     Ok(())
    // }
}
