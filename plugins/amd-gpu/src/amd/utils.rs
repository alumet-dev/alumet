use alumet::pipeline::elements::error::PollError;
use amdsmi::AmdsmiStatusT;
use log::error;
use std::{error::Error, fmt::Display};

/// Error treatment concerning AMD SMI library.
///
/// # Arguments
///
/// Take a status of [`AmdsmiStatusT`] provided by AMD SMI library to catch dynamically the occurred error.
#[derive(Debug)]
pub struct AmdError(pub AmdsmiStatusT);

impl Display for AmdError {
    fn fmt(&self, format: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(format, "amdsmi error {}", self.0)
    }
}

impl Error for AmdError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        None
    }
}

#[derive(Default)]
pub struct Available {
    /// GPU PCI bus ID identification feature validity.
    pub gpu_bus_id: bool,
    /// GPU energy consumption feature validity.
    pub gpu_energy_consumption: bool,
    /// GPU engine units usage (graphics, memory and average multimedia engines) feature validity.
    pub gpu_engine_usage: bool,
    /// GPU memory usage (VRAM, GTT) feature validity.
    pub gpu_memory_usages: bool,
    /// GPU electric power consumption feature validity.
    pub gpu_power_consumption: bool,
    /// GPU temperature feature validity.
    pub gpu_temperatures: bool,
    /// GPU power management feature validity.
    pub gpu_state_management: bool,
    /// Process counter feature validity.
    pub process_counter: bool,
    /// Process compute unit usage feature validity.
    pub process_compute_unit_usage: bool,
    /// Process VRAM memory usage feature validity.
    pub process_vram_usage: bool,
}

pub fn try_feature<T>(res: Result<T, AmdError>) -> Result<(bool, Option<T>), PollError> {
    match res {
        Ok(value) => Ok((true, Some(value))),
        Err(AmdError(e)) => {
            // Function to provide a metric not properly implemented
            if e == AmdsmiStatusT::AmdsmiStatusNotSupported || e == AmdsmiStatusT::AmdsmiStatusNotYetImplemented {
                error!("Feature not supported by AMD SMI : {e}");
                Ok((false, None))
            // Ressource is busy or retried to get metric value
            } else if e == AmdsmiStatusT::AmdsmiStatusRetry || e == AmdsmiStatusT::AmdsmiStatusBusy {
                return Err(PollError::CanRetry(anyhow::anyhow!(
                    "Retry to get AMD SMI metric : {e}"
                )));
            // Fatal errors status
            } else if e == AmdsmiStatusT::AmdsmiStatusTimeout || e == AmdsmiStatusT::AmdsmiStatusApiFailed {
                return Err(PollError::Fatal(anyhow::anyhow!("Fatal AMD SMI error : {e}")));
            // Other errors types
            } else {
                error!("Failed to get metric : {e}");
                Ok((true, None))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test `fmt` function in `Display` implementation for `AmdError` with AMD SMI error display
    #[test]
    fn test_fmt_display() {
        let error = AmdError(AmdsmiStatusT::AmdsmiStatusSuccess);
        let msg = format!("amdsmi error {}", error.0);
        assert_eq!(format!("{}", error), msg);
    }

    // Test `source` function in `Error` implementation for `AmdError`
    #[test]
    fn test_source() {
        let error = AmdError(AmdsmiStatusT::AmdsmiStatusSuccess);
        assert!(error.source().is_none());
    }

    // Test `try_feature` function without status on the retrieved value
    #[test]
    fn test_try_feature_success() {
        let value = 32u32;
        let res = try_feature::<u32>(Ok(value));
        assert!(matches!(res, Ok((true, Some(v))) if v == value));
    }

    // Test `try_feature` function for a feature not compatible with a device
    #[test]
    fn test_try_feature_not_supported_status() {
        let res = try_feature::<u32>(Err(AmdError(AmdsmiStatusT::AmdsmiStatusNotSupported)));
        assert!(matches!(res, Ok((false, None))));
    }

    // Test `try_feature` function with status compatible with retrying to get a value
    #[test]
    fn test_try_feature_retry_status() {
        let res = try_feature::<u32>(Err(AmdError(AmdsmiStatusT::AmdsmiStatusRetry)));
        assert!(matches!(res, Err(PollError::CanRetry(_))));
    }

    // Test `try_feature` function with blocking and fatals errors
    #[test]
    fn test_try_feature_fatal_status() {
        let res = try_feature::<u32>(Err(AmdError(AmdsmiStatusT::AmdsmiStatusTimeout)));
        assert!(matches!(res, Err(PollError::Fatal(_))));
    }

    // Test `try_feature` function with unknown status identified
    #[test]
    fn test_try_feature_unknown_status() {
        let res = try_feature::<u32>(Err(AmdError(AmdsmiStatusT::AmdsmiStatusUnknownError)));
        assert!(matches!(res, Ok((true, None))));
    }
}
