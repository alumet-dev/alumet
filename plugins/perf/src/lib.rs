#[cfg(not(target_os = "linux"))]
compile_error!("This plugin only works on Linux.");

mod event;
mod group;
mod multiplexing;
mod plugin;
mod resource;
mod sysfs;

pub use plugin::PerfPlugin;
