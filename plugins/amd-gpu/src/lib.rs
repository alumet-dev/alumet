use anyhow::{Context, anyhow};
use serde::{Deserialize, Serialize};
use std::{os::unix::process, time::Duration};

use alumet::{
    pipeline::elements::source::trigger::TriggerSpec,
    plugin::{
        ConfigTable,
        rust::{AlumetPlugin, deserialize_config, serialize_config},
    },
};

use rocm_smi_lib::*;

mod amd;
use amd::{device::AmdGpuDevices, error::AmdError, metrics::Metrics, probe::AmdGpuSource};

#[cfg(not(target_os = "linux"))]
compile_error!("This plugin only works on Linux.");

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Initial interval between two AMD GPU measurements.
    #[serde(with = "humantime_serde")]
    pub poll_interval: Duration,
    /// Initial interval between two flushing of AMD GPU measurements.
    #[serde(with = "humantime_serde")]
    pub flush_interval: Duration,
    /// On startup, the plugin inspects the GPU devices and detect their features.
    /// If `skip_failed_devices = true`, inspection failures will be logged and the plugin will continue.
    /// If `skip_failed_devices = false`, the first failure will make the plugin's startup fail.
    pub skip_failed_devices: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(1),
            flush_interval: Duration::from_secs(5),
            skip_failed_devices: true,
        }
    }
}
pub struct AmdGpuPlugin {
    pub config: Config,
}

const SENSORS: [RsmiTemperatureType; 7] = [
    RsmiTemperatureType::Edge,
    RsmiTemperatureType::Junction,
    RsmiTemperatureType::Memory,
    RsmiTemperatureType::Hbm0,
    RsmiTemperatureType::Hbm1,
    RsmiTemperatureType::Hbm2,
    RsmiTemperatureType::Hbm3,
];

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
    // Première récupération du nombre de processus
    let res = unsafe { rsmi_compute_process_info_get(std::ptr::null_mut(), &mut num_items) };
    if res != rsmi_status_t_RSMI_STATUS_SUCCESS {
        return Err(res);
    }
    if num_items == 0 {
        return Ok(Vec::new());
    }

    let mut process = Vec::with_capacity(num_items as usize);
    // Appel sécurisé du code externe pour remplir le vecteur
    let res = unsafe {
        // set_len est appelé une fois ici pour permettre l'écriture dans le buffer
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

fn test() -> Result<(), RocmErr> {
    let mut rocm = RocmSmi::init()?;
    let count = rocm.get_device_count();

    for _x in 0..50 {
        for dv_ind in 0..count {
            let device_id = rocm.get_device_identifiers(dv_ind)?.unique_id;
            let gpu_voltage_consumption = rocm.get_device_voltage_metric(dv_ind, RsmiVoltageMetric::Current);
            let gpu_memory_gtt_usage = rocm.get_device_memory_data(dv_ind)?.gtt_used;
            let gpu_memory_vram_usage = rocm.get_device_memory_data(dv_ind)?.vram_used;

            let metric = RsmiTemperatureMetric::Current;

            for sensor in SENSORS {
                match rocm.get_device_temperature_metric(dv_ind, sensor, metric) {
                    Ok(temperature) => println!("Temperature {:?} = {} °C", sensor, temperature),
                    Err(e) => eprintln!("Failed to get temperature for {:?}: {:?}", sensor, e),
                }
            }

            println!(
                "GPU: {}",
                device_id
                    .expect("Failed to get GPU device unique identification")
                    .to_string()
            );

            println!(
                "Voltage: {} mV",
                gpu_voltage_consumption.expect("Failed to get GPU current voltage")
            );

            println!("GTT: {} B", gpu_memory_gtt_usage);
            println!("VRAM: {} B", gpu_memory_vram_usage);

            match get_device_power(dv_ind) {
                Ok(power) => {
                    println!("Power: {} W", power / 1_000_000);
                }
                Err(e) => eprintln!("Error rsmi_dev_power_get function: {:?}", e),
            }

            match get_device_energy(dv_ind) {
                Ok((energy, resolution, _timestamp)) => {
                    println!("Energy: {} J", energy as f32 * resolution / 1e6);
                }
                Err(e) => eprintln!("Error rsmi_dev_energy_count_get function: {:?}", e),
            }

            match get_compute_process_info(dv_ind) {
                Ok(procs) => {
                    for (_i, p) in procs.iter().enumerate() {
                        let pid = p.process_id;
                        let vram_usage = p.vram_usage;
                        println!("pid: {pid} vram_usage: {vram_usage}");
                    }
                }
                Err(e) => eprintln!("Error rsmi_compute_process_info_get function: {e}"),
            }

            println!();
        }
    }
    Ok(())
}

impl AlumetPlugin for AmdGpuPlugin {
    // Name of plugin, in lowercase, without the "plugin-" prefix
    fn name() -> &'static str {
        "amdgpu"
    }

    // Gets the version from the Cargo.toml of the plugin crate
    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        let config = serialize_config(Config::default())?;
        Ok(Some(config))
    }

    // Initialization of AMD GPU and AMD SMI library.
    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config)?;
        Ok(Box::new(AmdGpuPlugin { config }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        test();

        /*let rocmsmi = AmdGpuDevices::detect(self.config.skip_failed_devices)?;
        let stats = rocmsmi.detection_stats();

        if stats.found_devices == 0 {
            return Err(anyhow!("No ROCMSMI-compatible GPU found."));
        }
        if stats.working_devices == 0 {
            return Err(anyhow!(
                "{} ROCMSMI-compatible devices found but none of them is working (see previous warnings).",
                stats.found_devices
            ));
        }

        for device in rocmsmi.devices.iter() {
            log::info!(
                "Found AMD GPU device {} with features: {}",
                device.bus_id,
                device.features
            );
        }

        let metrics = Metrics::new(alumet)?;
        for device in rocmsmi.devices.into_iter() {
            let source_name = format!("device_{}", device.bus_id);
            let source = AmdGpuSource::new(device, metrics.clone()).map_err(AmdError)?;
            let trigger = TriggerSpec::builder(self.config.poll_interval)
                .flush_interval(self.config.flush_interval)
                .build()?;
            alumet.add_source(&source_name, Box::new(source), trigger)?;
        }*/

        Ok(())
    }

    // Stop AMD GPU plugin and shut down the AMD SMI library.
    fn stop(&mut self) -> anyhow::Result<()> {
        unsafe { rsmi_shut_down() };
        Ok(())
    }
}
