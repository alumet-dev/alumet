#[cfg(not(target_os = "linux"))]
compile_error!("This plugin only works on Linux.");

mod cpuinfo;
mod formula;
mod plugin;

pub use cpuinfo::{CpuInfo, read_cpu_info};
pub use plugin::FlopsMemoryPlugin;
