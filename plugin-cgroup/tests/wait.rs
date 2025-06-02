use std::path::PathBuf;

use plugin_cgroup::{
    detect::{CgroupCallback, CgroupDetector},
    hierarchy::CgroupHierarchy,
    wait::MountWait,
};

#[test]
fn create_dirs_without_delay() {
    // let wait = MountWait::new(PathBuf::from("/sys/fs/cgroup"), |res| {
    //     match res {
    //         Ok(path) => {
    //             let h = CgroupHierarchy::new_at(path).unwrap();
    //             let handler = MyHandler;
    //             struct MyHandler;
    //             impl CgroupCallback for MyHandler {
    //                 fn on_new_cgroup(&mut self, cgroup: plugin_cgroup::Cgroup) {
    //                     todo!("créer la source")
    //                 }

    //                 fn on_error(&mut self, err: Box<dyn std::error::Error>) {
    //                     todo!("log error")
    //                 }
    //             }
    //             let detector = CgroupDetector::new(h, handler).unwrap();
    //         },
    //         Err(_) => todo!(),
    //     }

    // }, );
}
