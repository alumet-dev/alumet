use std::io::{self, ErrorKind};

use alumet::pipeline::elements::error::PollError;

/// Error code for "no such device".
const ENODEV: i32 = 19;

/// Turns an io result into a poll result, analyzing the error to determine
/// whether it is a "normal termination" or not.
pub fn analyze_io_result<R>(res: io::Result<R>) -> Result<R, PollError> {
    match res {
        Ok(value) => Ok(value),
        Err(e) if e.kind() == ErrorKind::NotFound || e.raw_os_error() == Some(ENODEV) => {
            // The cgroup is gone, the source should stop normally (expected situation).
            Err(PollError::NormalStop)
        }
        Err(e) => Err(PollError::Fatal(e.into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{read_to_string, write};

    #[test]
    fn test_analyze_io_result() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test_file");
        write(&path, "content").unwrap();
        let result = analyze_io_result(read_to_string(&path));
        assert!(result.is_ok());
    }

    #[test]
    fn test_analyze_io_result_invalid() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("invalid");
        let result = analyze_io_result(read_to_string(&path));
        assert!(matches!(result, Err(PollError::NormalStop)));
    }

    #[test]
    fn test_analyze_io_result_error() {
        let e = io::Error::new(ErrorKind::PermissionDenied, "error");
        let result = analyze_io_result::<()>(Err(e));
        assert!(matches!(result, Err(PollError::Fatal(_e))));
    }
}
