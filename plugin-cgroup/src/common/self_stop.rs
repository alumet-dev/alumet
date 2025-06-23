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
