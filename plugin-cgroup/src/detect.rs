//! Detection of cgroups.

use std::{
    error::Error,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use notify::{Watcher, event::CreateKind};
use rustc_hash::{FxBuildHasher, FxHashSet};
use walkdir::WalkDir;

use crate::hierarchy::{Cgroup, CgroupHierarchy, CgroupVersion};

pub struct CgroupDetector {
    // keeps the watcher alive
    #[allow(dead_code)]
    watcher: Box<dyn Watcher>,

    state: Arc<Mutex<DetectorState>>,
}

pub trait CgroupCallback: Send {
    fn on_new_cgroup(&mut self, cgroup: Cgroup);

    // TODO more precise error type
    fn on_error(&mut self, err: Box<dyn Error>);
}

struct DetectorState {
    /// A set of known cgroupfs, to avoid detecting the same group multiple times.
    /// Can be modified by the handler and by calling methods on `CgroupDetector`.
    known_cgroups: FxHashSet<String>,

    /// Callbacks that handle detection events.
    callbacks: Box<dyn CgroupCallback>,
}

#[derive(Clone)]
struct EventHandler {
    hierarchy: CgroupHierarchy,
    state: Arc<Mutex<DetectorState>>,
}

const INITIAL_CAPACITY: usize = 128;

impl CgroupDetector {
    pub fn new(hierarchy: CgroupHierarchy, handler: impl CgroupCallback + 'static) -> anyhow::Result<Self> {
        let state = Arc::new(Mutex::new(DetectorState {
            known_cgroups: FxHashSet::with_capacity_and_hasher(INITIAL_CAPACITY, FxBuildHasher),
            callbacks: Box::new(handler),
        }));

        let handler = EventHandler {
            hierarchy: hierarchy.clone(),
            state: state.clone(),
        };

        let mut watcher: Box<dyn Watcher> = match hierarchy.version() {
            CgroupVersion::V1 => {
                // inotify is not supported, use polling
                let config = notify::Config::default();
                let initial_scan_handler = handler.clone();

                // PollWatcher performs the initial scan on its own.
                let watcher = notify::PollWatcher::with_initial_scan(handler, config, initial_scan_handler)?;
                Box::new(watcher)
            }
            CgroupVersion::V2 => {
                // inotify is supported
                // First, start the watcher. Then, do the initial scan. This way, we will not miss events.
                let watcher = notify::recommended_watcher(handler.clone())?;

                // We need to manually do the initial scan.
                initial_scan(&hierarchy, handler);
                Box::new(watcher)
            }
        };
        watcher.watch(hierarchy.root(), notify::RecursiveMode::Recursive)?;

        Ok(Self { watcher, state })
    }

    pub fn is_known(&self, cgroup: &str) -> bool {
        self.state.lock().unwrap().known_cgroups.contains(cgroup)
    }

    pub fn forget(&mut self, cgroup: &str) -> bool {
        self.state.lock().unwrap().known_cgroups.remove(cgroup)
    }
}

fn initial_scan(hierarchy: &CgroupHierarchy, mut handler: EventHandler) {
    let mut initial_cgroup_paths = Vec::with_capacity(INITIAL_CAPACITY);
    for entry_res in WalkDir::new(hierarchy.root()) {
        match entry_res {
            Ok(entry) => {
                if entry.file_type().is_dir() {
                    initial_cgroup_paths.push(entry.into_path());
                }
            }
            Err(err) => handler.propagate_error(err),
        }
    }
    handler.register_cgroups(initial_cgroup_paths);
}

impl EventHandler {
    fn register_cgroups(&mut self, paths: Vec<PathBuf>) {
        let mut state = self.state.lock().unwrap();
        for path in paths {
            let cgroup = Cgroup::new(&self.hierarchy, path);
            if state.known_cgroups.insert(cgroup.cgroup_path().to_owned()) {
                // the set did not contain the value: this is a new cgroup
                state.callbacks.on_new_cgroup(cgroup);
            }
        }
    }

    fn propagate_error(&mut self, err: impl Error + 'static) {
        self.state.lock().unwrap().callbacks.on_error(Box::new(err));
    }
}

impl notify::EventHandler for EventHandler {
    fn handle_event(&mut self, event: notify::Result<notify::Event>) {
        match event {
            Ok(evt) => {
                if evt.kind == notify::EventKind::Create(CreateKind::Folder) {
                    self.register_cgroups(evt.paths);
                }
            }
            Err(err) => {
                self.propagate_error(err);
            }
        }
    }
}

impl notify::poll::ScanEventHandler for EventHandler {
    fn handle_event(&mut self, event: notify::poll::ScanEvent) {
        // TODO optimize: collect the paths first and then handle them all at once
        match event {
            Ok(path) => {
                if path.is_dir() {
                    self.register_cgroups(vec![path]);
                }
            }
            Err(err) => {
                self.propagate_error(err);
            }
        }
    }
}
