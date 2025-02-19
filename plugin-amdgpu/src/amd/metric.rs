use alumet::pipeline::elements::error::PollError;
use amdsmi::{
    amdsmi_get_clock_info, amdsmi_get_energy_count, amdsmi_get_gpu_activity, amdsmi_get_gpu_compute_process_info,
    amdsmi_get_gpu_compute_process_info_by_pid, amdsmi_get_gpu_device_bdf, amdsmi_get_gpu_fan_speed,
    amdsmi_get_gpu_memory_usage, amdsmi_get_gpu_pci_throughput, amdsmi_get_power_info, amdsmi_get_processor_handles,
    amdsmi_get_socket_handles, amdsmi_get_temp_metric, amdsmi_is_gpu_power_management_enabled, AmdsmiClkTypeT,
    AmdsmiMemoryTypeT, AmdsmiTemperatureMetricT, AmdsmiTemperatureTypeT,
};
use anyhow::Context;
use std::ffi::c_void;

use super::utils::{try_feature, AmdError, Available};

// Clock frequencies values available
const CLK_TYPE: [(AmdsmiClkTypeT, &str); 10] = [
    (AmdsmiClkTypeT::AmdsmiClkTypeSys, "frequency_system"),
    (AmdsmiClkTypeT::AmdsmiClkTypeDf, "frequency_display_factory"),
    (AmdsmiClkTypeT::AmdsmiClkTypeDcef, "frequency_display_controller_engine"),
    (AmdsmiClkTypeT::AmdsmiClkTypeSoc, "frequency_system_on_chip"),
    (AmdsmiClkTypeT::AmdsmiClkTypeMem, "frequency_memory"),
    (AmdsmiClkTypeT::AmdsmiClkTypePcie, "frequency_pci_bus"),
    (AmdsmiClkTypeT::AmdsmiClkTypeVclk0, "frequency_video_core_0"),
    (AmdsmiClkTypeT::AmdsmiClkTypeVclk1, "frequency_video_core_1"),
    (AmdsmiClkTypeT::AmdsmiClkTypeDclk0, "frequency_display_0"),
    (AmdsmiClkTypeT::AmdsmiClkTypeDclk1, "frequency_display_1"),
];

// Memories values available
const MEMORY_TYPE: [(AmdsmiMemoryTypeT, &str); 2] = [
    (AmdsmiMemoryTypeT::AmdsmiMemTypeGtt, "memory_graphic_translation_table"),
    (AmdsmiMemoryTypeT::AmdsmiMemTypeVram, "memory_video_computing"),
];

// Temperature sensors values available
const SENSOR_TYPE: [(AmdsmiTemperatureTypeT, &str); 8] = [
    (AmdsmiTemperatureTypeT::AmdsmiTemperatureTypeEdge, "thermal_global"),
    (AmdsmiTemperatureTypeT::AmdsmiTemperatureTypeHotspot, "thermal_hotspot"),
    (AmdsmiTemperatureTypeT::AmdsmiTemperatureTypeVram, "thermal_vram"),
    (
        AmdsmiTemperatureTypeT::AmdsmiTemperatureTypeHbm0,
        "thermal_high_bandwidth_memory_0",
    ),
    (
        AmdsmiTemperatureTypeT::AmdsmiTemperatureTypeHbm1,
        "thermal_high_bandwidth_memory_1",
    ),
    (
        AmdsmiTemperatureTypeT::AmdsmiTemperatureTypeHbm2,
        "thermal_high_bandwidth_memory_2",
    ),
    (
        AmdsmiTemperatureTypeT::AmdsmiTemperatureTypeHbm3,
        "thermal_high_bandwidth_memory_3",
    ),
    (AmdsmiTemperatureTypeT::AmdsmiTemperatureTypePlx, "thermal_pci_bus"),
];

/// Collect processes metrics running on AMD GPU.
#[derive(Default)]
pub struct AmdGpuProcessMetrics {
    /// Process ID.
    pub pid: Option<u32>,
    /// Process compute unit usage in percentage.
    pub compute_unit_usage: Option<u64>,
    /// Process counter.
    pub counter: Option<u64>,
    /// Process VRAM memory usage in MB.
    pub vram_usage: Option<u64>,
    /// Features validity.
    pub available: Available,
}

/// Structure to collect AMD GPU metrics.
#[derive(Default)]
pub struct AmdGpuMetrics {
    /// GPU identification by BDF PCIe bus.
    pub bus_id: Option<String>,
    /// GPU clock frequencies in Mhz.
    pub clock_frequencies: Vec<(Option<u64>, String)>,
    /// GPU energy consumption in mJ.
    pub energy_consumption: Option<u64>,
    /// GPU engine units usage (graphics, memory and average multimedia engines) in percentage.
    pub engine_usage: Option<u64>,
    /// GPU fan speed in percentage.
    pub fan_speed: Option<u64>,
    /// GPU memory usage (VRAM, GTT) in MB.
    pub memory_usages: Vec<(Option<u64>, String)>,
    /// GPU PCI bus sent data consumption in KB/s.
    pub pci_data_sent: Option<u64>,
    /// GPU PCI bus received data consumption in KB/s.
    pub pci_data_received: Option<u64>,
    /// GPU electric power consumption in W.
    pub power_consumption: Option<u64>,
    /// GPU temperature in °C.
    pub temperatures: Vec<(Option<u64>, String)>,
    /// GPU power management state.
    pub state_management: Option<bool>,
}

/// Define an AMD GPU device and its metrics.
pub struct AmdGpuDevice {
    /// AMD GPU recognition by bus identification.
    pub ptr: *mut c_void,
    /// AMD GPU collected metrics with [`AmdGpuMetrics`].
    pub metrics: AmdGpuMetrics,
    /// AMD GPU features validity.
    pub available: Available,
}

impl AmdGpuDevice {
    /// New [`AmdGpuDevice`] instance for each GPU device.
    fn new(ptr: *mut c_void) -> Self {
        Self {
            ptr,
            metrics: AmdGpuMetrics::default(),
            available: Available::default(),
        }
    }

    /// Collect the metrics for a given GPU device.
    ///
    /// # Return
    ///
    /// - A vector of [`AmdGpuMetrics`], to store available AMD GPU data.
    /// - An error from the Alumet pipeline if a critical metric is not found.
    fn gather_amd_gpu_measurements(&mut self) -> Result<(), PollError> {
        let device = self.ptr;

        // Get GPU device identification with Bus Device Function using the processor handle
        let (supported, feature) = try_feature(amdsmi_get_gpu_device_bdf(device).map_err(AmdError))?;
        self.available.gpu_bus_id = supported;
        if let Some(id) = feature {
            self.metrics.bus_id = Some(id.to_string());
        }

        // Get GPU energy consumption in Joules
        let (supported, feature) = try_feature(amdsmi_get_energy_count(device).map_err(AmdError))?;
        self.available.gpu_energy_consumption = supported;
        if let Some((energy, resolution, _timestamp)) = feature {
            self.metrics.energy_consumption = Some((energy * resolution as u64) / 1_000);
        }

        // Get the power management state for GPUs
        let (supported, feature) = try_feature(amdsmi_is_gpu_power_management_enabled(device).map_err(AmdError))?;
        self.available.gpu_state_management = supported;
        if let Some(state) = feature {
            self.metrics.state_management = Some(state);
            // Determine if power management is enable for GPUs
            if state {
                // Get average and current power consumption GPU in Watts
                let (supported, feature) = try_feature(amdsmi_get_power_info(device).map_err(AmdError))?;
                self.available.gpu_power_consumption = supported;
                if let Some(power) = feature {
                    self.metrics.power_consumption = Some(power.average_socket_power as u64);
                }
            }
        }

        // Get the GPU usage of hardware engine units by running graphic processes (GFX).
        let (supported, feature) = try_feature(amdsmi_get_gpu_activity(device).map_err(AmdError))?;
        self.available.gpu_engine_usage = supported;
        if let Some(engine) = feature {
            self.metrics.engine_usage = Some(engine.gfx_activity as u64);
        }

        // Get GPU fan speed in percentage
        let (supported, feature) = try_feature(amdsmi_get_gpu_fan_speed(device, 0).map_err(AmdError))?;
        self.available.gpu_fan_speed = supported;
        if let Some(fan) = feature {
            self.metrics.fan_speed = Some(fan as u64);
        }

        // Get GPU PCI bus data consumption
        let (supported, feature) = try_feature(amdsmi_get_gpu_pci_throughput(device).map_err(AmdError))?;
        self.available.gpu_pci_data_sent = supported;
        self.available.gpu_pci_data_received = supported;
        if let Some((sent, received, _max_pkt_sz)) = feature {
            self.metrics.pci_data_sent = Some(sent / 1_000);
            self.metrics.pci_data_received = Some(received / 1_000);
        }

        // Get GPU current clock frequencies metric by hardware sectors
        for (clktype, label) in &CLK_TYPE {
            let (supported, feature) = try_feature(amdsmi_get_clock_info(device, *clktype).map_err(AmdError))?;
            self.available.gpu_clock_frequencies |= supported;
            self.metrics
                .clock_frequencies
                .push((feature.map(|clock| clock.clk as u64), label.to_string()));
        }

        // Get GPU memories usage by types
        for (memtype, label) in &MEMORY_TYPE {
            let (supported, feature) = try_feature(amdsmi_get_gpu_memory_usage(device, *memtype).map_err(AmdError))?;
            self.available.gpu_memory_usages |= supported;
            self.metrics
                .memory_usages
                .push((feature.map(|memory| memory / 1_000_000), label.to_string()));
        }

        // Get GPU current temperatures metric by hardware sectors
        for (sensor, label) in &SENSOR_TYPE {
            let (supported, feature) = try_feature(
                amdsmi_get_temp_metric(device, *sensor, AmdsmiTemperatureMetricT::AmdsmiTempCurrent).map_err(AmdError),
            )?;
            self.available.gpu_temperatures |= supported;
            self.metrics
                .temperatures
                .push((feature.map(|temperature| temperature as u64), label.to_string()));
        }

        Ok(())
    }
}

/// Retrieve useful data metrics on GPUs AMD based models.
///
/// # Return
///
/// - A vector of [`AmdGpuDevice`], to store data concerning each AMD GPU installed on a machine.
/// - An error from the Alumet pipeline if a critical metric is not found.
pub fn gather_amd_gpu_device_measurements() -> Result<Vec<AmdGpuDevice>, PollError> {
    let mut devices = Vec::new();

    // Get socket handles
    let socket_handles = amdsmi_get_socket_handles()
        .map_err(AmdError)
        .context("Failed to get socket handles")?;

    for socket_handle in socket_handles {
        // Get processor handles for each socket handle
        let devices_handles = amdsmi_get_processor_handles(socket_handle)
            .map_err(AmdError)
            .context(format!("Failed to get processor handles for socket {socket_handle:?}"))?;

        for device_handle in devices_handles {
            let mut device = AmdGpuDevice::new(device_handle);
            device.gather_amd_gpu_measurements()?;
            devices.push(device);
        }
    }

    Ok(devices)
}

/// Retrieve useful data metrics on AMD GPU running processes.
///
/// # Return
///
/// - An [`AmdGpuProcessMetrics`] struture that storing data concerning catching AMD GPU process.
/// - An error from the Alumet pipeline if a critical metric is not found.
pub fn gather_amd_gpu_process_measurements() -> Result<Vec<AmdGpuProcessMetrics>, PollError> {
    let mut metrics = Vec::new();

    let (supported, feature) = try_feature(amdsmi_get_gpu_compute_process_info().map_err(AmdError))?;
    if !supported {
        return Ok(metrics);
    }

    if let Some((procs, num_items)) = feature {
        for proc in procs {
            let pid = proc.process_id;
            let (supported_pid, feature) =
                try_feature(amdsmi_get_gpu_compute_process_info_by_pid(pid).map_err(AmdError))?;

            if supported_pid {
                if let Some(prc) = feature {
                    let mut metric = AmdGpuProcessMetrics {
                        pid: Some(pid),
                        counter: Some(num_items as u64),
                        compute_unit_usage: Some(prc.cu_occupancy as u64),
                        vram_usage: Some(prc.vram_usage / 1_000_000),
                        available: Available::default(),
                    };

                    metric.available.process_counter = supported;
                    metric.available.process_compute_unit_usage = supported_pid;
                    metric.available.process_vram_usage = supported_pid;

                    metrics.push(metric);
                }
            }
        }
    }

    Ok(metrics)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test `gather_amd_gpu_device_metrics` function with no GPU available
    #[test]
    fn test_gather_amd_gpu_device_metrics_error() {
        let result = gather_amd_gpu_device_measurements();
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }
}
