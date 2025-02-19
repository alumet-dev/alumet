use alumet::pipeline::elements::error::PollError;
use amdsmi::*;
use anyhow::Context;
use std::ffi::c_void;

use super::utils::{is_valid, AmdError, Features};

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
pub struct AmdGpuProcessMetric {
    /// Process ID.
    pub pid: Option<u32>,
    /// Process compute unit usage in percentage.
    pub compute_unit_usage: Option<Features<u64>>,
    /// Process counter.
    pub counter: Option<Features<u64>>,
    /// Process VRAM memory usage in MB.
    pub vram_usage: Option<Features<u64>>,
}

/// Structure to collect AMD GPU metrics.
#[derive(Default)]
pub struct AmdGpuMetric {
    /// GPU identification by BDF PCIe bus.
    pub bus_id: Option<String>,
    /// GPU clock frequencies in Mhz.
    pub clock_frequency: Vec<(Features<u64>, String)>,
    /// GPU energy consumption in mJ.
    pub energy_consumption: Option<Features<u64>>,
    /// GPU engine units usage (graphics, memory and average multimedia engines) in percentage.
    pub engine_usage: Option<Features<u64>>,
    /// GPU memory usage (VRAM, GTT) in MB.
    pub memory_usage: Vec<(Features<u64>, String)>,
    /// GPU PCI bus sent data consumption in KB/s.
    pub pci_data_sent: Option<Features<u64>>,
    /// GPU PCI bus received data consumption in KB/s.
    pub pci_data_received: Option<Features<u64>>,
    /// GPU electric power consumption in W.
    pub power_consumption: Option<Features<u64>>,
    /// GPU temperature in °C.
    pub temperature: Vec<(Features<u64>, String)>,
}

/// Define an AMD GPU device and its metrics.
pub struct AmdGpuDevice {
    /// AMD GPU recognition by bus identification.
    pub ptr: *mut c_void,
    /// AMD GPU collected metrics with [`AmdGpuMetric`].
    pub metrics: AmdGpuMetric,
}

impl AmdGpuDevice {
    /// New [`AmdGpuDevice`] instance for each GPU device.
    fn new(ptr: *mut c_void) -> Self {
        Self {
            ptr,
            metrics: AmdGpuMetric::default(),
        }
    }

    /// Update the metrics for a given GPU device.
    ///
    /// # Return
    ///
    /// - A vector of [`AmdGpuMetric`], to store available AMD GPU data.
    /// - An error from the Alumet pipeline if a critical metric is not found.
    fn gather_amd_gpu_metrics(&mut self) -> Result<(), PollError> {
        let device = self.ptr;

        // Get GPU device identification with Bus Device Function using the processor handle
        let metric = amdsmi_get_gpu_device_bdf(device)
            .map_err(AmdError)
            .context("Failed to get GPU compute process information by PID")?;
        self.metrics.bus_id = Some(metric.to_string());

        // Get GPU energy consumption in Joules
        let metric = is_valid(|| amdsmi_get_energy_count(device).map_err(AmdError));
        if let Some((energy_accumulator, counter_resolution, _timestamp)) = metric.value {
            self.metrics.energy_consumption = Some(Features::new(
                Some((energy_accumulator * counter_resolution as u64) / 1_000),
                true,
            ));
        }

        // Get average and current power consumption GPU in Watts
        let metric = is_valid(|| amdsmi_get_power_info(device).map_err(AmdError));
        if let Some(data) = metric.value {
            self.metrics.power_consumption = Some(Features::new(Some(data.average_socket_power as u64), true));
        }

        // Get the GPU usage of hardware engine units by running graphic processes (GFX).
        let metric = is_valid(|| amdsmi_get_gpu_activity(device).map_err(AmdError));
        if let Some(data) = metric.value {
            self.metrics.engine_usage = Some(Features::new(Some(data.gfx_activity as u64), true));
        }

        // Get GPU PCI bus data consumption
        let metric = is_valid(|| amdsmi_get_gpu_pci_throughput(device).map_err(AmdError));
        if let Some((sent, received, _max_pkt_sz)) = metric.value {
            self.metrics.pci_data_sent = Some(Features::new(Some(sent / 1_000), true));
            self.metrics.pci_data_received = Some(Features::new(Some(received / 1_000), true));
        }

        // Get GPU current clock frequencies metric by hardware sectors
        for (clk, label) in &CLK_TYPE {
            let metric = is_valid(|| amdsmi_get_clock_info(device, *clk).map_err(AmdError));
            if let Some(data) = metric.value {
                self.metrics
                    .clock_frequency
                    .push((Features::new(Some(data.clk as u64), true), label.to_string()));
            }
        }

        // Get GPU memories usage
        for (mem, label) in &MEMORY_TYPE {
            let metric = is_valid(|| amdsmi_get_gpu_memory_usage(device, *mem).map_err(AmdError));
            if let Some(data) = metric.value {
                self.metrics
                    .memory_usage
                    .push((Features::new(Some(data / 1_000_000), true), label.to_string()));
            }
        }

        // Get GPU current temperatures metric by hardware sectors
        for (temp, label) in &SENSOR_TYPE {
            let metric = is_valid(|| {
                amdsmi_get_temp_metric(device, *temp, AmdsmiTemperatureMetricT::AmdsmiTempCurrent).map_err(AmdError)
            });
            if let Some(data) = metric.value {
                self.metrics
                    .temperature
                    .push((Features::new(Some(data as u64), true), label.to_string()));
            }
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
pub fn gather_amd_gpu_device_metrics() -> Result<Vec<AmdGpuDevice>, PollError> {
    let mut devices = Vec::new();

    // Get socket handles
    if let Err(_e) = amdsmi_get_socket_handles()
        .map_err(AmdError)
        .context("Failed to get socket handles")
    {
        // Shut down AMD SMI if no socket handles exists
        amdsmi_shut_down()
            .map_err(AmdError)
            .context("Failed to shut down AMD SMI")?;

        return Ok(devices);
    } else {
        for socket_handle in amdsmi_get_socket_handles().unwrap() {
            // Get processor handles for each socket handle
            let devices_handles = amdsmi_get_processor_handles(socket_handle)
                .map_err(AmdError)
                .context(format!("Failed to get processor handles for socket {socket_handle:?}"))?;

            for device_handle in devices_handles {
                let mut device = AmdGpuDevice::new(device_handle);
                device.gather_amd_gpu_metrics()?;
                devices.push(device);
            }
        }
    }
    Ok(devices)
}

/// Retrieve useful data metrics on AMD GPU running processes.
///
/// # Return
///
/// - An [`AmdGpuProcessMetric`] struture that storing data concerning catching AMD GPU process.
/// - An error from the Alumet pipeline if a critical metric is not found.
pub fn gather_amd_gpu_metric_process() -> Result<AmdGpuProcessMetric, PollError> {
    let mut metric = AmdGpuProcessMetric::default();

    // Get the number of running GPU compute process
    let data = is_valid(|| amdsmi_get_gpu_compute_process_info().map_err(AmdError));
    if let Some((procs, num_items)) = data.value {
        if num_items > 0 {
            for proc in procs {
                let pid = proc.process_id;

                // Get the GPU compute process information
                let item = is_valid(|| amdsmi_get_gpu_compute_process_info_by_pid(pid).map_err(AmdError));
                if let Some(data) = item.value {
                    metric.pid = Some(pid);
                    metric.counter = Some(Features::new(Some(num_items as u64), true));
                    metric.compute_unit_usage = Some(Features::new(Some(data.cu_occupancy as u64), true));
                    metric.vram_usage = Some(Features::new(Some(data.vram_usage / 1_000_000), true));
                }
            }
        }
    }

    Ok(metric)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test `gather_amd_gpu_device_metrics` function with no GPU available
    #[test]
    fn test_gather_amd_gpu_device_metrics_error() {
        let result = gather_amd_gpu_device_metrics();
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }
}
