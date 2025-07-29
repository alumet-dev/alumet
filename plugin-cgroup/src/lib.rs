// Prevent compiling outside of Linux: cgroups only exist on Linux, and we rely on Linux mechanisms like /proc/mounts, inotify and epoll.
#[cfg(not(target_os = "linux"))]
compile_error!("only Linux is supported");

pub mod plugins;
pub mod probe;
