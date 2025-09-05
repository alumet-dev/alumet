use rocm_smi_lib::*;
use std::fmt::Display;

// Temperature sensors values available
pub const SENSOR_TYPE: [(RsmiTemperatureType, &str); 7] = [
    (RsmiTemperatureType::Edge, "thermal_global"),
    (RsmiTemperatureType::Junction, "thermal_hotspot"),
    (RsmiTemperatureType::Memory, "thermal_vram"),
    (RsmiTemperatureType::Hbm0, "thermal_high_bandwidth_memory_0"),
    (RsmiTemperatureType::Hbm1, "thermal_high_bandwidth_memory_1"),
    (RsmiTemperatureType::Hbm1, "thermal_high_bandwidth_memory_2"),
    (RsmiTemperatureType::Hbm1, "thermal_high_bandwidth_memory_3"),
];

/// Indicates which features are available on a given ADM GPU device.
#[derive(Debug, Default)]
pub struct OptionalFeatures {
    /// GPU energy consumption feature validity.
    pub gpu_energy_consumption: bool,
    /// GPU memory usage (VRAM, GTT) feature validity.
    pub gpu_memory_usages: bool,
    /// GPU electric power consumption feature validity.
    pub gpu_power_consumption: bool,
    /// GPU temperature feature validity.
    pub gpu_temperatures: Vec<(RsmiTemperatureType, bool)>,
    /// GPU socket voltage consumption feature validity.
    pub gpu_voltage_consumption: bool,
    /// GPU process info feature validity.
    pub gpu_process_info: bool,
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

/// Checks if a feature is supported by the available GPU by inspecting the return type of an ROCMSMI function.
pub fn is_supported<T>(res: Result<T, RocmErr>) -> Result<bool, RocmErr> {
    match res {
        Ok(_) => Ok(true),
        Err(RocmErr::RsmiStatusPermission) => Ok(false),
        Err(RocmErr::RsmiStatusNotSupported) => Ok(false),
        Err(RocmErr::RsmiStatusNotYetImplemented) => Ok(false),
        Err(RocmErr::RsmiStatusUnexpectedData) => Ok(false),
        Err(e) => Err(e),
    }
}

impl OptionalFeatures {
    /// Detect the features available on the given device.
    pub fn detect_on(mut rocm: RocmSmi, device: u32) -> Result<Self, RocmErr> {
        let mut gpu_temperatures = Vec::new();
        for &(sensor, _) in &SENSOR_TYPE {
            let supported = is_supported(rocm.get_device_temperature_metric(device, sensor, RsmiTemperatureMetric::Current))?;
            gpu_temperatures.push((sensor, supported));
        }
    
        Ok(Self {
            gpu_energy_consumption: is_supported(Ok(get_device_energy(device)))?,
            gpu_power_consumption: is_supported(Ok(get_device_power(device)))?,
            gpu_memory_usages: is_supported(rocm.get_device_memory_data(device))?,
            gpu_voltage_consumption: is_supported(rocm.get_device_voltage_metric(device, RsmiVoltageMetric::Current))?,
            gpu_process_info: is_supported(Ok(get_compute_process_info(device)))?,
            gpu_temperatures,
        })
    }

    // Test and return the availability of feature on a given
    pub fn with_detected_features(device: RocmSmiDevice) -> Result<(RocmSmiDevice, Self), RocmErr> {
        Self::detect_on(&device).map(|features| (device, features))
    }

    pub fn has_any(&self) -> bool {
        !(!self.gpu_energy_consumption
            && !self.gpu_power_consumption
            && !self.gpu_memory_usages
            && !self.gpu_process_info
            && !self.gpu_voltage_consumption
            && !self.gpu_temperatures.iter().any(|&(_sensor, supported)| supported))
    }
}

impl Display for OptionalFeatures {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut available = Vec::new();

        if self.gpu_energy_consumption {
            available.push("gpu_energy_consumption".to_string());
        }
        if self.gpu_power_consumption {
            available.push("gpu_power_consumption".to_string());
        }
        if self.gpu_process_info {
            available.push("gpu_process_info".to_string());
        }
        if self.gpu_memory_usages {
            available.push("gpu_memory_usages".to_string());
        }
        for (temp_type, supported) in &self.gpu_temperatures {
            if *supported {
                available.push(format!("gpu_temperatures::{temp_type:?}"));
            }
        }

        write!(f, "{}", available.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mock optional features
    fn mock_optional_features() -> OptionalFeatures {
        let mut gpu_temperatures = Vec::new();
        for (sensor_type, _) in &SENSOR_TYPE {
            gpu_temperatures.push((*sensor_type, false));
        }

        OptionalFeatures {
            gpu_energy_consumption: false,
            gpu_memory_usages: false,
            gpu_power_consumption: false,
            gpu_voltage_consumption: false,
            gpu_temperatures,
            gpu_process_info: false,
        }
    }

    // Test `fmt` function
    #[test]
    fn test_fmt_sucess() {
        let mut features = mock_optional_features();

        features.gpu_energy_consumption = true;
        features.gpu_power_consumption = true;
        features.gpu_process_info = true;
        features.gpu_memory_usages = true;
        features.gpu_temperatures.push((RsmiTemperatureType::Edge, true));

        assert_eq!(
            format!("{features}"),
            "gpu_energy_consumption, gpu_power_consumption, gpu_process_info, gpu_memory_usages, gpu_temperatures::Edge"
        );
    }

    // Test `is_supported` function with identified AmdsmiStatusT errors to disable a feature
    #[test]
    fn test_is_supported_errors() {
        let errors = [
            RocmErr::RsmiStatusPermission,
            RocmErr::RsmiStatusNotSupported,
            RocmErr::RsmiStatusNotYetImplemented,
            RocmErr::RsmiStatusUnexpectedData,
        ];
        for &err in &errors {
            let ret: Result<i32, RocmErr> = Err(err);
            let res = is_supported(ret).unwrap();
            assert!(!res);
        }
    }

    // Test `is_supported` function with other AmdsmiStatusT errors
    #[test]
    fn test_is_supported_other_error() {
        let err = RocmErr::RsmiStatusUnknownError;
        let ret: Result<i32, RocmErr> = Err(err);
        let res = is_supported(ret);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), err);
    }

    // Test `has_any` function with no feature available
    #[test]
    fn test_has_any_all_false() {
        let features = OptionalFeatures {
            gpu_energy_consumption: false,
            gpu_memory_usages: false,
            gpu_power_consumption: false,
            gpu_voltage_consumption: false,
            gpu_temperatures: HashMap::new(),
            gpu_process_info: false,
        };
        assert!(!features.has_any());
    }
}
