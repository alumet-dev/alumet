use std::path::PathBuf;

pub mod inotify;

pub enum RecursiveMode {
    Recursive,
    Flat,
}

// NOTE: unlike the `notify` crate, this module forces the watches to be added before the watcher is fully built.
// This allows to simplify the implementation of the watcher.

pub trait Watcher {
    fn stop(&mut self) -> anyhow::Result<()>;
}

#[derive(Debug)]
pub enum Event {
    NeedRescan,
    Fs {
        created: Vec<(PathBuf, PathKind)>,
        deleted: Vec<(PathBuf, PathKind)>,
    },
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum PathKind {
    File,
    Directory,
}

pub trait EventHandler: Send {
    fn handle_event(&mut self, event: anyhow::Result<Event>);
}
