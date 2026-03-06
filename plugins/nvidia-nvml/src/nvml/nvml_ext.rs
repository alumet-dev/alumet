//! "Extends" the nvml_wrapper crate to fix some functions.

use nvml_wrapper::{Device, error::NvmlError, struct_wrappers::device::ProcessUtilizationSample};

/// Extension trait for NVML `Device`.
pub trait DeviceExt {
    fn fixed_process_utilization_stats(
        &self,
        last_seen_timestamp: u64,
    ) -> Result<Vec<ProcessUtilizationSample>, NvmlError>;
}

impl DeviceExt for Device<'_> {
    /// Gets utilization stats for relevant currently running processes.
    ///
    /// Utilization stats are returned for processes that had a non-zero utilization stat at some point during the target sample period.
    /// See [`Device::process_utilization_stats`] for more information.
    ///
    /// This wrapper fixes the error handling of the function.
    fn fixed_process_utilization_stats(
        &self,
        last_seen_timestamp: u64,
    ) -> Result<Vec<ProcessUtilizationSample>, NvmlError> {
        match Device::process_utilization_stats(self, last_seen_timestamp) {
            // NotFound can happen if there is no sample between now and the timestamp, in particular when the machine has just started.
            Err(NvmlError::NotFound) => Ok(vec![]),
            res => res,
        }
    }
}
