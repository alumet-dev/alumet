use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint, Timestamp},
    pipeline::{Source, elements::error::PollError},
    plugin::util::CounterDiff,
    resources::{Resource, ResourceConsumer},
};
use anyhow::Result;

use rocm_smi_lib::*;

use super::{device::ManagedDevice, error::AmdError, features::SENSOR_TYPE, metrics::Metrics};

/// Measurement source that queries AMD GPU devices.
pub struct AmdGpuSource {
    /// Internal state to compute the difference between two increments of the counter.
    energy_counter: CounterDiff,
    /// Handle to the GPU, with features information.
    device: ManagedDevice,
    /// Alumet metrics IDs.
    metrics: Metrics,
    /// Alumet resource ID.
    resource: Resource,
}

/// SAFETY: The amd libary is thread-safe and returns pointers to a safe global state, which we can pass to other threads.
unsafe impl Send for ManagedDevice {}

impl AmdGpuSource {
    pub fn new(device: ManagedDevice, metrics: Metrics) -> Result<AmdGpuSource, RocmErr> {
        let bus_id = std::borrow::Cow::Owned(device.bus_id.clone());
        Ok(AmdGpuSource {
            energy_counter: CounterDiff::with_max_value(u64::MAX),
            device,
            metrics,
            resource: Resource::Gpu { bus_id },
        })
    }
}

/// Call an unsafe C binding function to retrieves [`RSMI_POWER_TYPE`] power values.
///
/// # Arguments
///
/// - `dv_ind` : Index of a device
///
/// # Returns
///
/// - `power`: Pointer for C binding function, to allow it to allocate memory to get its corresponding value.
/// - An error if we can't to retrieve the value, and had [`rsmi_status_t_RSMI_STATUS_SUCCESS`] status.
fn get_device_power(dv_ind: u32) -> Result<u64, rsmi_status_t> {
    let mut power = 0;
    let mut type_ = RSMI_POWER_TYPE::default();

    let result = unsafe { rsmi_dev_power_get(dv_ind, &mut power as *mut u64, &mut type_ as *mut _) };

    if result == rsmi_status_t_RSMI_STATUS_SUCCESS {
        Ok(power)
    } else {
        Err(result)
    }
}

/// Call an unsafe C binding function to retrieves energy values
///
/// # Arguments
///
/// - `dv_ind` : Index of a device
///
/// # Returns
///
/// - `energy`: Pointer for C binding function, to allow it to allocate memory to get its corresponding value.
/// - `resolution`: Resolution precision of the energy counter in micro Joules.
/// - `timestamp`: Timestamp returned in ns.
/// - An error if we can't to retrieve the value, and had [`rsmi_status_t_RSMI_STATUS_SUCCESS`] status.
fn get_device_energy(dv_ind: u32) -> Result<(u64, f32, u64), rsmi_status_t> {
    let mut energy = 0;
    let mut resolution = 0.0;
    let mut timestamp = 0;

    let result = unsafe {
        rsmi_dev_energy_count_get(
            dv_ind,
            &mut energy as *mut u64,
            &mut resolution as *mut f32,
            &mut timestamp as *mut u64,
        )
    };

    if result == rsmi_status_t_RSMI_STATUS_SUCCESS {
        Ok((energy, resolution, timestamp))
    } else {
        Err(result)
    }
}

/// Get process count
///
/// # Arguments
///
/// - `dv_ind` : Index of a device
fn get_compute_process_info(dv_ind: u32) -> Result<Vec<rsmi_process_info_t>, rsmi_status_t> {
    let mut num_items: u32 = 0;
    let res = unsafe { rsmi_compute_process_info_get(std::ptr::null_mut(), &mut num_items) };
    if res != rsmi_status_t_RSMI_STATUS_SUCCESS {
        return Err(res);
    }
    if num_items == 0 {
        return Ok(Vec::new());
    }

    let mut process = Vec::with_capacity(num_items as usize);
    let res = unsafe {
        process.set_len(num_items as usize);
        rsmi_compute_process_info_get(process.as_mut_ptr(), &mut num_items)
    };
    if res != rsmi_status_t_RSMI_STATUS_SUCCESS {
        return Err(res);
    }

    unsafe {
        process.set_len(num_items as usize);
    }

    let mut result = Vec::with_capacity(num_items as usize);
    for p in &process {
        let pid = p.process_id;
        let mut proc_ = unsafe { std::mem::zeroed() };

        let res = unsafe { rsmi_compute_process_info_by_device_get(pid, dv_ind, &mut proc_) };
        if res == rsmi_status_t_RSMI_STATUS_SUCCESS {
            result.push(proc_);
        } else {
            eprintln!("Error process: PID {pid} code {res}");
        }
    }

    Ok(result)
}

impl Source for AmdGpuSource {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        let features = &self.device.features;
        let handle = self.device.handle;

        // no consumer, we just monitor the device here
        let consumer = ResourceConsumer::LocalMachine;

        // GPU energy consumption metric pushed
        if features.gpu_energy_consumption
            && let Ok((energy, resolution, _timestamp)) = get_device_energy(handle)
        {
            let diff = self.energy_counter.update(energy).difference();
            if let Some(value) = diff {
                measurements.push(MeasurementPoint::new(
                    timestamp,
                    self.metrics.gpu_energy_consumption,
                    self.resource.clone(),
                    consumer.clone(),
                    (value as f64 * resolution as f64) / 1e3,
                ));
            }
        }

        // GPU instant electric power consumption metric pushed
        if features.gpu_power_consumption
            && let Ok(value) = amdsmi_get_power_info(handle).map_err(AmdError)
        {
            measurements.push(MeasurementPoint::new(
                timestamp,
                self.metrics.gpu_power_consumption,
                self.resource.clone(),
                consumer.clone(),
                value.average_socket_power as u64,
            ));
        }

        // GPU memories used metric pushed
        if features.gpu_memory_usages {
            measurements.push(MeasurementPoint::new(
                timestamp,
                self.metrics.gpu_memory_gtt_usage,
                self.resource.clone(),
                consumer.clone(),
                rocm.get_device_memory_data(dv_ind)?.gtt_used,
            ));
            measurements.push(MeasurementPoint::new(
                timestamp,
                self.metrics.gpu_memory_vram_usage,
                self.resource.clone(),
                consumer.clone(),
                rocm.get_device_memory_data(dv_ind)?.vram_used,
            ));
        }

        // GPU temperatures metric pushed
        for (sensor, label) in &SENSOR_TYPE {
            if features.gpu_temperatures.iter()
            .find(|(s, _)| (*s as u32) == (*sensor as u32))
            .map(|(_, v)| *v)
            .unwrap_or(false) {

                if let Ok(value) = rocm.get_device_temperature(handle, *sensor, RsmiTemperatureMetric::Current)? {
                    measurements.push(
                        MeasurementPoint::new(
                            timestamp,
                            self.metrics.gpu_temperatures,
                            self.resource.clone(),
                            consumer.clone(),
                            value as u64,
                        )
                        .with_attr("thermal_zone", label.to_string()),
                    );
                }
            } 
        }

        // Push GPU compute-graphic process informations if processes existing
        if features.gpu_process_info
            && let Ok(process_list) = amdsmi::amdsmi_get_gpu_process_list(handle).map_err(AmdError)
        {
            for process in process_list {
                let consumer = ResourceConsumer::Process { pid: process.pid };

                // Process VRAM memory usage
                measurements.push(MeasurementPoint::new(
                    timestamp,
                    self.metrics.process_memory_usage_vram,
                    self.resource.clone(),
                    consumer.clone(),
                    process.memory_usage.vram_mem,
                ));
            }
        }

        Ok(())
    }
}
