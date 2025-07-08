use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use util_cgroups::file_watch::{inotify::InotifyWatcher, Event, EventHandler, PathKind};

#[test]
fn test_detection() -> anyhow::Result<()> {
    const TOLERANCE: Duration = Duration::from_millis(250);
    let tmp = tempfile::tempdir()?;
    let event_handler = InotifyEventCheck::new();
    let events = Arc::clone(&event_handler.events);

    let watcher = InotifyWatcher::new(event_handler, vec![tmp.path().to_owned()])?;

    // create some directories
    std::fs::create_dir(tmp.path().join("dir1"))?;
    std::fs::create_dir(tmp.path().join("dir2"))?;
    std::fs::create_dir(tmp.path().join("dir3"))?;

    // check that they have been detected
    std::thread::sleep(TOLERANCE);
    check_events(
        "create",
        &events,
        vec![
            (tmp.path().join("dir1"), PathKind::Directory),
            (tmp.path().join("dir2"), PathKind::Directory),
            (tmp.path().join("dir3"), PathKind::Directory),
        ],
        vec![],
    );

    // more modifications
    std::fs::create_dir(tmp.path().join("dir1/subdir"))?;
    std::fs::create_dir(tmp.path().join("dir2/subdir"))?;
    std::fs::remove_dir(tmp.path().join("dir3"))?;

    // check that they have been detected
    std::thread::sleep(TOLERANCE);
    check_events(
        "mixed",
        &events,
        vec![
            (tmp.path().join("dir1/subdir"), PathKind::Directory),
            (tmp.path().join("dir2/subdir"), PathKind::Directory),
        ],
        vec![(tmp.path().join("dir3"), PathKind::Directory)],
    );

    // more modifications
    std::fs::remove_dir(tmp.path().join("dir1/subdir"))?;
    std::fs::remove_dir_all(tmp.path().join("dir2"))?;

    std::thread::sleep(TOLERANCE);
    check_events(
        "remove",
        &events,
        vec![],
        vec![
            (tmp.path().join("dir1/subdir"), PathKind::Directory),
            (tmp.path().join("dir2/subdir"), PathKind::Directory),
            (tmp.path().join("dir2"), PathKind::Directory),
        ],
    );

    // no notification should be generated after the stop
    watcher.stop_and_join()?;
    std::fs::create_dir(tmp.path().join("bad"))?;
    std::fs::create_dir(tmp.path().join("bad2"))?;
    std::fs::remove_dir(tmp.path().join("bad"))?;
    check_events("stopped", &events, vec![], vec![]);
    Ok(())
}

#[test]
fn test_stop() -> anyhow::Result<()> {
    const TOLERANCE: Duration = Duration::from_millis(250);
    let event_handler = InotifyEventCheck::new();
    let watcher = InotifyWatcher::new(event_handler, vec![])?;

    std::thread::sleep(TOLERANCE);
    watcher.stop_and_join().unwrap();
    Ok(())
}

fn check_events(
    label: &str,
    events: &Arc<Mutex<Vec<anyhow::Result<Event>>>>,
    mut expect_created: Vec<(PathBuf, PathKind)>,
    mut expect_deleted: Vec<(PathBuf, PathKind)>,
) {
    let events: Vec<_> = events.lock().unwrap().drain(..).collect();
    let mut all_created = Vec::new();
    let mut all_deleted = Vec::new();
    for evt in events {
        let evt = evt.expect("unexpected error in event processing");
        match evt {
            Event::NeedRescan => panic!("unexpected event NeedRescan"),
            Event::Fs { created, deleted } => {
                all_created.extend(created);
                all_deleted.extend(deleted);
            }
        }
    }
    all_created.sort_by_key(|(path, _)| path.as_os_str().to_owned());
    all_deleted.sort_by_key(|(path, _)| path.as_os_str().to_owned());
    expect_created.sort_by_key(|(path, _)| path.as_os_str().to_owned());
    expect_deleted.sort_by_key(|(path, _)| path.as_os_str().to_owned());
    assert_eq!(expect_created, all_created, "bad created events ({label})");
    assert_eq!(expect_deleted, all_deleted, "bad deleted events ({label})");
}

struct InotifyEventCheck {
    events: Arc<Mutex<Vec<anyhow::Result<Event>>>>,
}

impl InotifyEventCheck {
    fn new() -> Self {
        Self {
            events: Default::default(),
        }
    }
}

impl EventHandler for InotifyEventCheck {
    fn handle_event(&mut self, event: anyhow::Result<Event>) {
        self.events.lock().unwrap().push(event);
    }
}
