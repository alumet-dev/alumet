use std::fmt::Display;

pub mod detect;
pub mod features;
pub mod nvml_ext;
pub mod objects;

pub type NvmlResult<T> = Result<T, nvml_wrapper::error::NvmlError>;

pub trait NvmlProvider {
    type Lib: NvmlLib;

    fn get() -> anyhow::Result<Self::Lib>;
}

#[cfg_attr(test, mockall::automock(type Device = MockNvmlDevice;))]
pub trait NvmlLib {
    type Device: NvmlDevice;

    fn device_count(&self) -> anyhow::Result<u32>;
    fn device_by_index(&self, i: u32) -> anyhow::Result<Self::Device>;
}

#[cfg_attr(test, mockall::automock)]
pub trait NvmlDevice: Display + Send {
    /// Name of the device.
    /// See [`nvml_wrapper::Device::name`].
    fn name(&self) -> &str;

    /// PCI identifier of the device.
    /// See [`nvml_wrapper::PciInfo`].
    fn bus_id(&self) -> &str;

    /// Energy consumed since the last driver reload, in millijoules.
    /// See [`nvml_wrapper::Device::total_energy_consumption`].
    fn total_energy_consumption(&self) -> NvmlResult<u64>;

    /// Power usage in Watts.
    /// See [`nvml_wrapper::Device::power_usage`].
    fn power_usage(&self) -> NvmlResult<u32>;

    /// Temperature in °C.
    /// See [`nvml_wrapper::Device::temperature`].
    fn temperature(&self, sensor: nvml_wrapper::enum_wrappers::device::TemperatureSensor) -> NvmlResult<u32>;

    /// Current utilization rates.
    /// See [`nvml_wrapper::Device::utilization_rates`].
    fn utilization_rates(&self) -> NvmlResult<nvml_wrapper::struct_wrappers::device::Utilization>;

    //TODO handle memory info v1 (for older versions of NVML)
    /// Current memory usage.
    /// See [`nvml_wrapper::Device::memory_info`].
    fn memory_info(&self) -> NvmlResult<nvml_wrapper::struct_wrappers::device::MemoryInfo>;

    /// Utilization and sampling size of the decoder.
    /// See [`nvml_wrapper::Device::decoder_utilization`].
    fn decoder_utilization(&self) -> NvmlResult<nvml_wrapper::structs::device::UtilizationInfo>;

    /// Utilization and sampling size of the encoder.
    /// See [`nvml_wrapper::Device::encoder_utilization`].
    fn encoder_utilization(&self) -> NvmlResult<nvml_wrapper::structs::device::UtilizationInfo>;

    /// (last version) Number of processes with a **compute** context running on this Device.
    /// See [`nvml_wrapper::Device::running_compute_processes_count`].
    fn running_compute_processes_count(&self) -> NvmlResult<u32>;

    /// (old v2) Number of processes with a **compute** context running on this Device.
    /// See [`nvml_wrapper::Device::running_compute_processes_count_v2`].
    fn running_compute_processes_count_v2(&self) -> NvmlResult<u32>;

    /// (last version) Information about processes with a compute context running on this Device.
    /// See [`nvml_wrapper::Device::running_compute_processes`].
    fn running_compute_processes(&self) -> NvmlResult<Vec<nvml_wrapper::struct_wrappers::device::ProcessInfo>>;

    /// (old v2) Information about processes with a compute context running on this Device.
    /// See [`nvml_wrapper::Device::running_compute_processes`].
    fn running_compute_processes_v2(&self) -> NvmlResult<Vec<nvml_wrapper::struct_wrappers::device::ProcessInfo>>;

    /// (last version) Number of processes with a **graphics** context running on this Device.
    /// See [`nvml_wrapper::Device::running_graphics_processes_count`].
    fn running_graphics_processes_count(&self) -> NvmlResult<u32>;

    /// (old v2) Number of processes with a **graphics** context running on this Device.
    /// See [`nvml_wrapper::Device::running_graphics_processes_count_v2`].
    fn running_graphics_processes_count_v2(&self) -> NvmlResult<u32>;

    /// (last version) Information about processes with a graphics context running on this Device.
    /// See [`nvml_wrapper::Device::running_graphics_processes`].
    fn running_graphics_processes(&self) -> NvmlResult<Vec<nvml_wrapper::struct_wrappers::device::ProcessInfo>>;

    /// (old v2) Information about processes with a graphics context running on this Device.
    /// See [`nvml_wrapper::Device::running_graphics_processes`].
    fn running_graphics_processes_v2(&self) -> NvmlResult<Vec<nvml_wrapper::struct_wrappers::device::ProcessInfo>>;

    /// Gets utilization stats for relevant currently running processes.
    ///
    /// Utilization stats are returned for processes that had a non-zero utilization stat at some point during the target sample period.
    /// See [`nvml_wrapper::Device::process_utilization_stats`] for more information.
    fn process_utilization_stats(
        &self,
        last_seen_timestamp: u64,
    ) -> NvmlResult<Vec<nvml_wrapper::struct_wrappers::device::ProcessUtilizationSample>>;
}

#[cfg(test)]
impl Display for MockNvmlDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "mock {self:?}")
    }
}
