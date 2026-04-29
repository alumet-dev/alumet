//! Objects of the NVML library, wrapped in convenient structs.

use crate::nvml::NvmlProvider;

use super::{NvmlDevice, NvmlLib, NvmlResult};
use anyhow::Context;
use nvml_wrapper::{Device, Nvml, enum_wrappers::device::TemperatureSensor, struct_wrappers::device::ProcessInfo};
use nvml_wrapper_sys::bindings::nvmlDevice_t;
use std::{fmt::Display, sync::Arc};

pub struct NvmlLoader;

impl NvmlProvider for NvmlLoader {
    type Lib = ManagedNvml;

    fn get() -> anyhow::Result<Self::Lib> {
        let lib = Nvml::init().context("failed to load nvml")?;
        Ok(ManagedNvml(Arc::new(lib)))
    }
}

#[derive(Clone)]
pub struct ManagedNvml(Arc<Nvml>);

/// An NVML device that has been probed for available features.
pub struct ManagedDevice {
    /// The library must be initialized and alive (not dropped), otherwise the handle will no longer work.
    /// We use an Arc to ensure this in a way that's more easy for us than a lifetime on the struct.
    lib: ManagedNvml,
    /// A pointer to the device, as returned by NVML.
    handle: nvmlDevice_t,
    /// PCI bus ID of the device.
    bus_id: String,
    /// Name of the device.
    name: String,
}

// SAFETY: this struct is safe to move between threads, in particular because nvmlDevice_t is thread-safe.
unsafe impl Send for ManagedDevice {}

impl NvmlLib for ManagedNvml {
    type Device = ManagedDevice;

    fn device_count(&self) -> anyhow::Result<u32> {
        self.0.device_count().context("could not get device count")
    }

    fn device_by_index(&self, i: u32) -> anyhow::Result<ManagedDevice> {
        let device = self
            .0
            .device_by_index(i)
            .with_context(|| format!("could not get GPU device {i}"))?;
        let name = device
            .name()
            .with_context(|| format!("could not get the name of GPU device {i}"))?;
        let pci_info = device
            .pci_info()
            .with_context(|| format!("could not get PCI info for GPU device {i} \"{name}\""))?;
        let bus_id = pci_info.bus_id;

        // SAFETY: we will stop using the device before we drop the nvml library.
        let handle = unsafe { device.handle() };

        Ok(ManagedDevice {
            lib: self.clone(),
            handle,
            bus_id,
            name,
        })
    }
}

impl ManagedDevice {
    /// Returns a [`Device`] that provides NVML methods.
    fn as_underlying_device(&self) -> Device<'_> {
        // SAFETY: as long as `self` is alive, the nvml library is alive, so it's okay to create and use the device
        unsafe { Device::new(self.handle, &self.lib.0) }
    }
}

impl NvmlDevice for ManagedDevice {
    /// Name of the device.
    /// See [`nvml_wrapper::Device::name`].
    fn name(&self) -> &str {
        &self.name
    }

    /// PCI identifier of the device.
    /// See [`nvml_wrapper::PciInfo`].
    fn bus_id(&self) -> &str {
        &self.bus_id
    }

    /// Energy consumed since the last driver reload, in millijoules.
    /// See [`nvml_wrapper::Device::total_energy_consumption`].
    fn total_energy_consumption(&self) -> NvmlResult<u64> {
        self.as_underlying_device().total_energy_consumption()
    }

    /// Power usage in Watts.
    /// See [`nvml_wrapper::Device::power_usage`].
    fn power_usage(&self) -> NvmlResult<u32> {
        self.as_underlying_device().power_usage()
    }

    /// Temperature in °C.
    /// See [`nvml_wrapper::Device::temperature`].
    fn temperature(&self, sensor: TemperatureSensor) -> NvmlResult<u32> {
        self.as_underlying_device().temperature(sensor)
    }

    /// Current utilization rates.
    /// See [`nvml_wrapper::Device::utilization_rates`].
    fn utilization_rates(&self) -> NvmlResult<nvml_wrapper::struct_wrappers::device::Utilization> {
        self.as_underlying_device().utilization_rates()
    }

    /// Current memory usage.
    /// See [`nvml_wrapper::Device::memory_info`].
    fn memory_info(&self) -> NvmlResult<nvml_wrapper::struct_wrappers::device::MemoryInfo> {
        self.as_underlying_device().memory_info()
    }

    /// Utilization and sampling size of the decoder.
    /// See [`nvml_wrapper::Device::decoder_utilization`].
    fn decoder_utilization(&self) -> NvmlResult<nvml_wrapper::structs::device::UtilizationInfo> {
        self.as_underlying_device().decoder_utilization()
    }

    /// Utilization and sampling size of the encoder.
    /// See [`nvml_wrapper::Device::encoder_utilization`].
    fn encoder_utilization(&self) -> NvmlResult<nvml_wrapper::structs::device::UtilizationInfo> {
        self.as_underlying_device().encoder_utilization()
    }

    /// (last version) Number of processes with a **compute** context running on this Device.
    /// See [`nvml_wrapper::Device::running_compute_processes_count`].
    fn running_compute_processes_count(&self) -> NvmlResult<u32> {
        self.as_underlying_device().running_compute_processes_count()
    }

    /// (old v2) Number of processes with a **compute** context running on this Device.
    /// See [`nvml_wrapper::Device::running_compute_processes_count_v2`].
    fn running_compute_processes_count_v2(&self) -> NvmlResult<u32> {
        self.as_underlying_device().running_compute_processes_count_v2()
    }

    /// (last version) Information about processes with a compute context running on this Device.
    /// See [`nvml_wrapper::Device::running_compute_processes`].
    fn running_compute_processes(&self) -> NvmlResult<Vec<ProcessInfo>> {
        self.as_underlying_device().running_compute_processes()
    }

    /// (old v2) Information about processes with a compute context running on this Device.
    /// See [`nvml_wrapper::Device::running_compute_processes`].
    fn running_compute_processes_v2(&self) -> NvmlResult<Vec<ProcessInfo>> {
        self.as_underlying_device().running_compute_processes_v2()
    }

    /// (last version) Number of processes with a **graphics** context running on this Device.
    /// See [`nvml_wrapper::Device::running_graphics_processes_count`].
    fn running_graphics_processes_count(&self) -> NvmlResult<u32> {
        self.as_underlying_device().running_graphics_processes_count()
    }

    /// (old v2) Number of processes with a **graphics** context running on this Device.
    /// See [`nvml_wrapper::Device::running_graphics_processes_count_v2`].
    fn running_graphics_processes_count_v2(&self) -> NvmlResult<u32> {
        self.as_underlying_device().running_graphics_processes_count_v2()
    }

    /// (last version) Information about processes with a graphics context running on this Device.
    /// See [`nvml_wrapper::Device::running_graphics_processes`].
    fn running_graphics_processes(&self) -> NvmlResult<Vec<ProcessInfo>> {
        self.as_underlying_device().running_graphics_processes()
    }

    /// (old v2) Information about processes with a graphics context running on this Device.
    /// See [`nvml_wrapper::Device::running_graphics_processes`].
    fn running_graphics_processes_v2(&self) -> NvmlResult<Vec<ProcessInfo>> {
        self.as_underlying_device().running_graphics_processes_v2()
    }

    /// Gets utilization stats for relevant currently running processes.
    ///
    /// Utilization stats are returned for processes that had a non-zero utilization stat at some point during the target sample period.
    /// See [`nvml_wrapper::Device::process_utilization_stats`] for more information.
    fn process_utilization_stats(
        &self,
        last_seen_timestamp: u64,
    ) -> NvmlResult<Vec<nvml_wrapper::struct_wrappers::device::ProcessUtilizationSample>> {
        use super::nvml_ext::DeviceExt;
        self.as_underlying_device()
            .fixed_process_utilization_stats(last_seen_timestamp)
    }
}

impl Display for ManagedDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "device {} \"{}\"", self.bus_id, self.name)
    }
}
