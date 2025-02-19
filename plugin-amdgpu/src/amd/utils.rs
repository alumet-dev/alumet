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
    /// GPU clock frequencies feature validity.
    pub gpu_clock_frequencies: bool,
    /// GPU energy consumption feature validity.
    pub gpu_energy_consumption: bool,
    /// GPU engine units usage (graphics, memory and average multimedia engines) feature validity.
    pub gpu_engine_usage: bool,
    // GPU fan speed feature validity.
    pub gpu_fan_speed: bool,
    /// GPU memory usage (VRAM, GTT) feature validity.
    pub gpu_memory_usages: bool,
    /// GPU PCI bus sent data consumption feature validity.
    pub gpu_pci_data_sent: bool,
    /// GPU PCI bus received data consumption feature validity.
    pub gpu_pci_data_received: bool,
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

impl Available {
    pub fn has_any(&self) -> bool {
        self.gpu_bus_id
            && self.gpu_clock_frequencies
            && self.gpu_energy_consumption
            && self.gpu_engine_usage
            && self.gpu_fan_speed
            && self.gpu_memory_usages
            && self.gpu_power_consumption
            && self.gpu_temperatures
            && self.gpu_state_management
            && self.process_counter
            && self.process_compute_unit_usage
            && self.process_vram_usage
    }
}

pub fn try_feature<T>(res: Result<T, AmdError>) -> Result<(bool, Option<T>), PollError> {
    match res {
        Ok(value) => Ok((true, Some(value))),
        Err(AmdError(e)) => {
            // Function to provide a metric too new not properly implemented
            if e == AmdsmiStatusT::AmdsmiStatusNotSupported || e == AmdsmiStatusT::AmdsmiStatusNotYetImplemented {
                error!("Feature not supported by AMD SMI : {e}");
                Ok((false, None))
            // Ressource is busy or retried to get metric value
            } else if e == AmdsmiStatusT::AmdsmiStatusRetry || e == AmdsmiStatusT::AmdsmiStatusBusy {
                return Err(PollError::CanRetry(anyhow::anyhow!(
                    "Retry to get AMD SMI metric : {e}"
                )));
            // Fatal errors Considered status
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
}
