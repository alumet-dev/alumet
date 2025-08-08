use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use util_cgroups::file_watch::{inotify::InotifyWatcher, Event, EventHandler, PathKind};

const TOLERANCE: Duration = Duration::from_millis(250);

fn check_events(
    label: &str,
    events: &Arc<Mutex<Vec<anyhow::Result<Event>>>>,
    mut expect_created: Vec<(PathBuf, PathKind)>,
    mut expect_deleted: Vec<(PathBuf, PathKind)>,
) {
    use pretty_assertions::assert_eq;

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

#[test]
fn test_detection() -> anyhow::Result<()> {
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

/// Hardcoded alternative to `TempDir`.
///
/// Use this when you want to manually inspect the files produced by the test.
///
/// # Example
/// ```
/// let user_dir = std::env::var("HOME").unwrap();
/// let tmp = HardcodedDir(PathBuf::from(format!("{user_dir}/Documents/tests/inotify")));
/// let path = tmp.path();
/// ```
#[allow(unused)]
struct HardcodedDir(PathBuf);

#[allow(unused)]
impl HardcodedDir {
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

#[test]
fn file() -> anyhow::Result<()> {
    let _ = env_logger::try_init_from_env(env_logger::Env::default());

    let tmp = tempfile::tempdir()?;
    let event_handler = InotifyEventCheck::new();
    let events = Arc::clone(&event_handler.events);

    let _watcher = InotifyWatcher::new(event_handler, vec![tmp.path().to_owned()])?;

    // create some directories
    std::fs::create_dir_all(tmp.path().join("dir1/sub/little"))?;
    std::fs::write(tmp.path().join("dir1/sub/little/some_file.tmp"), "")?;

    // check that they have been detected
    std::thread::sleep(TOLERANCE);
    check_events(
        "create_subdirs_and_files",
        &events,
        vec![
            (tmp.path().join("dir1"), PathKind::Directory),
            (tmp.path().join("dir1/sub"), PathKind::Directory),
            (tmp.path().join("dir1/sub/little"), PathKind::Directory),
            (tmp.path().join("dir1/sub/little/some_file.tmp"), PathKind::File),
        ],
        vec![],
    );

    // more modifications
    std::fs::remove_file(tmp.path().join("dir1/sub/little/some_file.tmp"))?;

    // check that they have been detected
    std::thread::sleep(TOLERANCE);
    check_events(
        "create_delete_subdirs",
        &events,
        vec![],
        vec![(tmp.path().join("dir1/sub/little/some_file.tmp"), PathKind::File)],
    );

    Ok(())
}

#[test]
fn recursive_tricky() -> anyhow::Result<()> {
    let _ = env_logger::try_init_from_env(env_logger::Env::default());

    let tmp = tempfile::tempdir()?;
    let event_handler = InotifyEventCheck::new();
    let events = Arc::clone(&event_handler.events);

    let _watcher = InotifyWatcher::new(event_handler, vec![tmp.path().to_owned()])?;

    // create some directories
    std::fs::create_dir_all(tmp.path().join("dir1/sub/little/leaf"))?;
    std::fs::create_dir_all(tmp.path().join("dir2/sub/BIG"))?;
    std::fs::create_dir(tmp.path().join("dir3"))?;
    std::thread::sleep(TOLERANCE);
    std::fs::write(tmp.path().join("dir1/sub/little/some_file.tmp"), "")?;

    // check that they have been detected
    std::thread::sleep(TOLERANCE);
    check_events(
        "create_subdirs_and_files",
        &events,
        vec![
            (tmp.path().join("dir1"), PathKind::Directory),
            (tmp.path().join("dir1/sub"), PathKind::Directory),
            (tmp.path().join("dir1/sub/little"), PathKind::Directory),
            (tmp.path().join("dir1/sub/little/leaf"), PathKind::Directory),
            (tmp.path().join("dir1/sub/little/some_file.tmp"), PathKind::File),
            (tmp.path().join("dir2"), PathKind::Directory),
            (tmp.path().join("dir2/sub"), PathKind::Directory),
            (tmp.path().join("dir2/sub/BIG"), PathKind::Directory),
            (tmp.path().join("dir3"), PathKind::Directory),
        ],
        vec![],
    );

    // more modifications
    std::fs::remove_file(tmp.path().join("dir1/sub/little/some_file.tmp"))?;
    std::fs::create_dir_all(tmp.path().join("dir1/subdir/sousmarin"))?;
    std::thread::sleep(TOLERANCE);
    std::fs::remove_dir(tmp.path().join("dir1/subdir/sousmarin"))?;

    // check that they have been detected
    std::thread::sleep(TOLERANCE);
    check_events(
        "create_delete_subdirs",
        &events,
        vec![
            (tmp.path().join("dir1/subdir"), PathKind::Directory),
            (tmp.path().join("dir1/subdir/sousmarin"), PathKind::Directory),
        ],
        vec![
            (tmp.path().join("dir1/subdir/sousmarin"), PathKind::Directory),
            (tmp.path().join("dir1/sub/little/some_file.tmp"), PathKind::File),
        ],
    );

    Ok(())
}

#[test]
fn recursive_already_existing() -> anyhow::Result<()> {
    let _ = env_logger::try_init_from_env(env_logger::Env::default());

    let tmp = tempfile::tempdir()?;

    // create some directories BEFORE starting the watcher
    std::fs::create_dir_all(tmp.path().join("dir1/sub/little"))?;
    std::fs::create_dir_all(tmp.path().join("dir2/sub/BIG"))?;
    std::fs::create_dir(tmp.path().join("dir3"))?;

    let event_handler = InotifyEventCheck::new();
    let events = Arc::clone(&event_handler.events);

    let _watcher = InotifyWatcher::new(event_handler, vec![tmp.path().to_owned()])?;

    // check that the existing directories are not reported as detected
    std::thread::sleep(TOLERANCE);
    check_events("existing", &events, vec![], vec![]);

    // now, create some subdirs and subfiles
    std::fs::create_dir(tmp.path().join("dir1/sub/little/leaf"))?;
    std::fs::write(tmp.path().join("dir1/sub/little/some_file.tmp"), "")?;

    // check that they have been detected
    std::thread::sleep(TOLERANCE);
    check_events(
        "created_subdirs_and_files",
        &events,
        vec![
            (tmp.path().join("dir1/sub/little/leaf"), PathKind::Directory),
            (tmp.path().join("dir1/sub/little/some_file.tmp"), PathKind::File),
        ],
        vec![],
    );

    // remove and check
    std::fs::remove_dir(tmp.path().join("dir1/sub/little/leaf"))?;
    std::fs::remove_file(tmp.path().join("dir1/sub/little/some_file.tmp"))?;
    std::thread::sleep(TOLERANCE);
    check_events(
        "removed_subdirs_and_files",
        &events,
        vec![],
        vec![
            (tmp.path().join("dir1/sub/little/leaf"), PathKind::Directory),
            (tmp.path().join("dir1/sub/little/some_file.tmp"), PathKind::File),
        ],
    );

    // create again and check
    std::fs::create_dir(tmp.path().join("dir1/sub/little/leaf"))?;
    std::fs::write(tmp.path().join("dir1/sub/little/some_file.tmp"), "")?;
    std::thread::sleep(TOLERANCE);
    check_events(
        "created_subdirs_and_files(2)",
        &events,
        vec![
            (tmp.path().join("dir1/sub/little/leaf"), PathKind::Directory),
            (tmp.path().join("dir1/sub/little/some_file.tmp"), PathKind::File),
        ],
        vec![],
    );

    // remove the parent directory and all its content
    std::fs::remove_dir_all(tmp.path().join("dir1/sub"))?;
    std::thread::sleep(TOLERANCE);
    check_events(
        "removed_subdirs_rec",
        &events,
        vec![],
        vec![
            (tmp.path().join("dir1/sub"), PathKind::Directory),
            (tmp.path().join("dir1/sub/little"), PathKind::Directory),
            (tmp.path().join("dir1/sub/little/leaf"), PathKind::Directory),
            (tmp.path().join("dir1/sub/little/some_file.tmp"), PathKind::File),
        ],
    );
    Ok(())
}

#[test]
fn recursive_bad_root_inexistent() -> anyhow::Result<()> {
    let _ = env_logger::try_init_from_env(env_logger::Env::default());

    let tmp = tempfile::tempdir()?;
    let inexistent = tmp.path().join("i do not exist");

    // start the watcher on the bad root
    let event_handler = InotifyEventCheck::new();
    let res = InotifyWatcher::new(event_handler, vec![inexistent]);
    assert!(
        res.is_err(),
        "InotifyWatcher::new should fail because the root path does not exist"
    );
    Ok(())
}

#[test]
fn recursive_bad_root_file() -> anyhow::Result<()> {
    let _ = env_logger::try_init_from_env(env_logger::Env::default());

    let tmp = tempfile::tempdir()?;
    let file = tmp.path().join("i am a file");
    std::fs::write(&file, "")?;

    // start the watcher on the bad root
    let event_handler = InotifyEventCheck::new();
    let res = InotifyWatcher::new(event_handler, vec![file]);
    assert!(
        res.is_err(),
        "InotifyWatcher::new should fail because the root path is not a directory"
    );
    Ok(())
}
