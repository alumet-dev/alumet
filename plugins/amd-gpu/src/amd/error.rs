use rocm_smi_lib::*;
use std::{error::Error, fmt::Display};

/// Error treatment concerning AMD SMI library.
///
/// # Arguments
///
/// Take a status of [`AmdsmiStatusT`] provided by AMD SMI library to catch dynamically the occurred error.
#[derive(Debug)]
pub struct AmdError(pub RocmErr);

impl Display for AmdError {
    fn fmt(&self, format: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(format, "rocmsmi error {:?}", self.0)
    }
}

impl Error for AmdError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Test `fmt` function in `Display` implementation for `AmdError` with AMD SMI error display
    #[test]
    fn test_fmt_display() {
        let error = AmdError(RocmErr::RsmiStatusSuccess);
        let msg = format!("rocmsmi error {:?}", error.0);
        assert_eq!(format!("{error}"), msg);
    }

    // Test `source` function in `Error` implementation for `AmdError`
    #[test]
    fn test_source() {
        let error = AmdError(RocmErr::RsmiStatusSuccess);
        assert!(error.source().is_none());
    }
}
