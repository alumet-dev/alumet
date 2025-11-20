use amd_smi_lib_sys::bindings::amdsmi_status_t;
use std::{error::Error, fmt::Display};

/// Error treatment concerning AMD SMI library.
///
/// # Arguments
///
/// Take a status of [`amdsmi_status_t`] provided by AMD SMI library to catch dynamically the occurred error.
#[derive(Debug)]
pub struct AmdError(pub amdsmi_status_t);

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

#[cfg(test)]
mod tests {
    use amd_smi_lib_sys::bindings::amdsmi_status_t_AMDSMI_STATUS_SUCCESS;

    use super::*;
    // Test `fmt` function in `Display` implementation for `AmdError` with AMD SMI error display
    #[test]
    fn test_fmt_display() {
        let error = AmdError(amdsmi_status_t_AMDSMI_STATUS_SUCCESS);
        let msg = format!("amdsmi error {}", error.0);
        assert_eq!(format!("{error}"), msg);
    }

    // Test `source` function in `Error` implementation for `AmdError`
    #[test]
    fn test_source() {
        let error = AmdError(amdsmi_status_t_AMDSMI_STATUS_SUCCESS);
        assert!(error.source().is_none());
    }
}
