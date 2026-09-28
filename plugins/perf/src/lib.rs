#[cfg(not(target_os = "linux"))]
compile_error!("This plugin only works on Linux.");

pub mod event;
pub mod group;
mod multiplexing;
mod plugin;
pub mod resource;
mod sysfs;

pub use plugin::PerfPlugin;
