//! Detection of cgroups.

use std::{
    fmt::Debug,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{anyhow, Context};
use notify::{
    event::{CreateKind, RemoveKind},
    Watcher,
};
use rustc_hash::{FxBuildHasher, FxHashMap, FxHashSet};
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

/// A callback that reacts to the detection of cgroups by a [`CgroupDetector`].
pub trait Callback: Send {
    /// Called when new cgroups are detected.
    fn on_cgroups_created(&mut self, cgroups: Vec<Cgroup>) -> anyhow::Result<()>;

    /// Called when new cgroups are removed.
    fn on_cgroups_removed(&mut self, cgroups: Vec<Cgroup>) -> anyhow::Result<()>;
}

/// An easy way to create a `Callback` from two closures.
pub struct ClosureCallbacks<F1, F2>
where
    F1: for<'all> FnMut(Vec<Cgroup<'all>>) -> anyhow::Result<()> + Send,
    F2: for<'all> FnMut(Vec<Cgroup<'all>>) -> anyhow::Result<()> + Send,
{
    on_cgroups_created: F1,
    on_cgroups_removed: F2,
}

impl<F1, F2> Callback for ClosureCallbacks<F1, F2>
where
    F1: for<'all> FnMut(Vec<Cgroup<'all>>) -> anyhow::Result<()> + Send,
    F2: for<'all> FnMut(Vec<Cgroup<'all>>) -> anyhow::Result<()> + Send,
{
    fn on_cgroups_created(&mut self, cgroups: Vec<Cgroup>) -> anyhow::Result<()> {
        (self.on_cgroups_created)(cgroups)
    }

    fn on_cgroups_removed(&mut self, cgroups: Vec<Cgroup>) -> anyhow::Result<()> {
        (self.on_cgroups_removed)(cgroups)
    }
}

// Type inference for closures does not infer for<'a> but a specific lifetime, which doesn't work for
// implementing Callback. This helper forces the correct type.
//
/// Helper to build a callback.
pub fn callback<F: for<'all> FnMut(Vec<Cgroup<'all>>) -> anyhow::Result<()> + Send>(f: F) -> F {
    f
}

/// Internal state of the detector.
///
/// It is shared between the handler and the detector struct, to allow cgroups to be
/// added by the handler and forgotten by the detector.
struct DetectorState {
    /// A set of known cgroupfs, to avoid detecting the same group multiple times.
    /// Can be modified by the handler and by calling methods on `CgroupDetector`.
    known_cgroups_paths: FxHashSet<String>,

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
            known_cgroups_paths: FxHashSet::with_capacity_and_hasher(INITIAL_CAPACITY, FxBuildHasher),
            callback: Box::new(handler),
        }));

        let mut handler = EventHandler {
            hierarchy: hierarchy.clone(),
            state: state.clone(),
        };

        let mut watcher: Box<dyn Watcher + Send> = match hierarchy.version() {
            CgroupVersion::V1 => {
                // inotify is not supported, use polling
                let watcher_config = notify::Config::default();
                watcher_config.with_poll_interval(config.v1_refresh_interval);

                // PollWatcher performs the initial scan on its own.
                let initial_scan_handler = handler.clone();
                let watcher = notify::PollWatcher::with_initial_scan(handler, watcher_config, initial_scan_handler)?;
                Box::new(watcher)
            }
            CgroupVersion::V2 => {
                // inotify is supported
                // First, start the watcher. Then, do the initial scan. This way, we will not miss events.
                let watcher = notify::recommended_watcher(handler.clone())?;

                // We need to manually do the initial scan.
                let res = handler.initial_scan().context("error during initial scan");
                handler.handle_result(res);

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
        self.state.lock().unwrap().known_cgroups_paths.contains(cgroup)
    }

    /// Forgets a cgroup.
    ///
    /// If a control group with the same path is created in the future,
    /// it will trigger the callback again.
    pub fn forget(&mut self, cgroup: &str) -> bool {
        self.state.lock().unwrap().known_cgroups_paths.remove(cgroup)
    }
}

impl EventHandler {
    /// Registers new control groups.
    fn register_cgroups(&mut self, paths: Vec<PathBuf>) -> anyhow::Result<()> {
        // For optimization purposes, we register multiple cgroups at once,
        // so that we only need to lock() once.
        let mut state = self.state.lock().unwrap();
        let mut new_cgroups = Vec::with_capacity(paths.len());
        for path in paths {
            let cgroup = Cgroup::from_fs_path(&self.hierarchy, path);
            if state.known_cgroups_paths.insert(cgroup.canonical_path().to_owned()) {
                // the set did not contain the value: this is a new cgroup
                new_cgroups.push(cgroup);
            }
        }
        state.callback.on_cgroups_created(new_cgroups)
    }

    /// Removes control groups.
    fn forget_cgroups(&mut self, paths: Vec<PathBuf>) -> anyhow::Result<()> {
        let mut state = self.state.lock().unwrap();
        let mut removed = Vec::with_capacity(paths.len());
        for path in paths {
            let cgroup = Cgroup::from_fs_path(&self.hierarchy, path);
            if state.known_cgroups_paths.remove(cgroup.canonical_path()) {
                removed.push(cgroup);
            } else {
                // the set did not contain the value: weird
                log::warn!("tried to forget cgroup {cgroup} but it was not in the map");
            }
        }
        state.callback.on_cgroups_removed(removed)
    }

    /// Updates the list of control groups, removing old cgroups and registering new ones.
    fn update_cgroups(&mut self, paths: Vec<PathBuf>) -> anyhow::Result<()> {
        let mut state = self.state.lock().unwrap();
        let previously_known = &state.known_cgroups_paths;
        let mut current_cgroups = FxHashMap::with_capacity_and_hasher(paths.len(), FxBuildHasher);
        for path in paths {
            let cgroup = Cgroup::from_fs_path(&self.hierarchy, path);
            current_cgroups.insert(cgroup.canonical_path().to_owned(), cgroup);
        }

        // TODO use extract_if after Rust version upgrade (1.88)
        // Find new cgroups: in current_cgroups but not previously known
        let mut new_cgroups = Vec::default();
        for cgroup in current_cgroups.values() {
            if !previously_known.contains(cgroup.canonical_path()) {
                new_cgroups.push(cgroup.to_owned());
            }
        }
        // Find removed cgroups: previously known but not in current_cgroups
        let mut removed_cgroups = Vec::default();
        for cgroup_path in previously_known {
            if !current_cgroups.contains_key(cgroup_path) {
                let cgroup = Cgroup::from_fs_path(&self.hierarchy, cgroup_path.into());
                removed_cgroups.push(cgroup);
            }
        }

        // update list of known cgroups
        state.known_cgroups_paths = current_cgroups.into_keys().collect();

        // call the callbacks
        state
            .callback
            .on_cgroups_removed(removed_cgroups)
            .context("error in callback on_cgroups_removed")?;
        state
            .callback
            .on_cgroups_created(new_cgroups)
            .context("error in callback on_cgroups_created")?;
        // TODO always try both and combine errors
        Ok(())
    }

    fn handle_error(&mut self, err: impl Debug) {
        log::error!("error in event handler: {err:?}");
    }

    fn handle_result(&mut self, res: anyhow::Result<()>) {
        if let Err(err) = res {
            self.handle_error(err);
        }
    }

    /// Performs an initial scan of the cgroup hierarchy, and call the handler for each cgroup found.
    fn initial_scan(&mut self) -> anyhow::Result<()> {
        let initial_cgroup_paths = self.full_scan();
        self.register_cgroups(initial_cgroup_paths)
    }

    /// Rescans the cgroup hierarchy, removing the cgroups that no longer exist and registering the new ones.
    fn rescan(&mut self) -> anyhow::Result<()> {
        let paths = self.full_scan();
        self.update_cgroups(paths)
    }

    /// Performs a full recursive scan of the cgroup hierarchy and returns the cgroups found.
    fn full_scan(&mut self) -> Vec<PathBuf> {
        let mut cgroup_paths = Vec::with_capacity(INITIAL_CAPACITY);
        for entry_res in WalkDir::new(self.hierarchy.root()) {
            match entry_res {
                Ok(entry) => {
                    if entry.file_type().is_dir() {
                        cgroup_paths.push(entry.into_path());
                    }
                }
                Err(err) => self.handle_error(err),
            }
        }
        cgroup_paths
    }
}

impl notify::EventHandler for EventHandler {
    fn handle_event(&mut self, event: notify::Result<notify::Event>) {
        // TODO we get a lot of Access(Open(Any)) and Modify(Metadata(Any)) events, can we ignore them at the inotify level instead of in the match below?
        // -> yes, but we have to use inotify directly, not the notify rust wrapper… -> later
        match event {
            Ok(evt) => {
                let res = match evt.kind {
                    // TODO notify returns CreateKind::Any instead of CreateKind::Folder with the PollWatcher…
                    notify::EventKind::Create(CreateKind::Folder) => self.register_cgroups(evt.paths),
                    notify::EventKind::Create(CreateKind::Any) => {
                        // the PollWatcher (used for cgroup v1) returns CreateKind::Any events, we have to check the type manually…
                        let dir_paths = evt.paths.into_iter().filter(|p| p.is_dir()).collect();
                        self.register_cgroups(dir_paths)
                    }
                    notify::EventKind::Remove(RemoveKind::Folder) => self.forget_cgroups(evt.paths),
                    notify::EventKind::Remove(RemoveKind::Any) => {
                        // We cannot use is_dir() in the filter because the file or folder no longer exists.
                        // Use a simplistic test that is not perfect but should work.
                        // It's okay to call forget_cgroups with a path that was not registered before, it will
                        // be ignored (or just print a warning).
                        let dir_paths = evt
                            .paths
                            .into_iter()
                            .filter(|p| {
                                p.extension()
                                    .is_none_or(|ext| ext != "stat" && !ext.as_encoded_bytes().ends_with(b"_in_bytes"))
                            })
                            .collect();
                        self.forget_cgroups(dir_paths)
                    }
                    notify::EventKind::Other if evt.flag() == Some(notify::event::Flag::Rescan) => {
                        // Some events have been lost, we must recheck everything.
                        self.rescan()
                    }
                    _ => Ok(()),
                };
                self.handle_result(res);
            }
            Err(err) => {
                self.handle_error(err);
            }
        };
    }
}

impl notify::poll::ScanEventHandler for EventHandler {
    fn handle_event(&mut self, event: notify::poll::ScanEvent) {
        // TODO optimize: collect the paths first and then handle them all at once
        // But this is only used with cgroup v1 so it's fine for now…
        match event {
            Ok(path) => {
                if path.is_dir() {
                    let res = self.register_cgroups(vec![path]);
                    self.handle_result(res);
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
