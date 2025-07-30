//! Watch processes through it's pid
use anyhow::Context;
use std::{fs, io, path::PathBuf, ptr, time::Duration};
use thiserror::Error;

use crate::pipeline::{control::request, matching::SourceNamePattern, MeasurementPipeline};

use super::{builder::ShutdownError, RunningAgent};

/// Error that can occur in [`watch_process`].
#[derive(Error, Debug)]
pub enum WatchError {
    /// The process could not be spawned.
    #[error("failed to check process with pid {0}")]
    PidCheck(String, #[source] std::num::ParseIntError),
    /// The process has spawned but waiting for it has failed.
    #[error("failed to wait for pid {0}")]
    ProcessWait(i32, #[source] std::io::Error),
    /// An error occurred while waiting for the agent to shut down.
    #[error("error in shutdown")]
    Shutdown(#[source] ShutdownError),
}

const TRIGGER_TIMEOUT: Duration = Duration::from_secs(1);

/// Watch process that runs identified by it's pid until it's end.
///
/// The measurement sources are triggered before the process spawns and after it exits.
///
/// After the process exits, the pipeline must stop within `shutdown_timeout`, or an error is returned.
pub fn watch_process(agent: RunningAgent, pid: String, shutdown_timeout: Duration) -> Result<(), WatchError> {
    // Check if we can convert the pid to i332
    let pid_u32 = pid
        .parse::<i32>()
        .map_err(|e| WatchError::PidCheck(pid.to_string(), e))?;

    if let Err(e) = trigger_measurement_now(&agent.pipeline) {
        log::error!("Could not trigger a first measurement before the child spawn: {e}");
    }

    // Spawn the process and wait for it to exit.
    wait_child(pid_u32)?;
    log::info!("Watched process exited, Alumet will now stop.");

    // One last measurement.
    if let Err(e) = trigger_measurement_now(&agent.pipeline) {
        log::error!("Could not trigger one last measurement after the child exit: {e}");
    }

    // Stop the pipeline
    agent.pipeline.control_handle().shutdown();
    agent.wait_for_shutdown(shutdown_timeout).map_err(WatchError::Shutdown)
}

/// Spawns a child process and waits for it to exit.
fn wait_child(pid: i32) -> Result<(), WatchError> {
    let pidfd = unsafe { nc::pidfd_open(pid, 0) };
    if pidfd == Err(nc::errno::ENOSYS) {
        log::warn!("PIDFD_OPEN syscall not supported in this system use of another way");
        let path = PathBuf::from(format!("/proc/{pid}/"));

        loop {
            if !fs::metadata(&path).is_ok() {
                break;
            }
            // Wait 5s to avoid too much load
            std::thread::sleep(std::time::Duration::from_secs(5));
        }
        return Ok(());
    }
    let pidfd = match pidfd {
        Ok(pidfd) => pidfd,
        Err(e) => {
            log::error!("Can't look for the process, exiting now");
            return Err(WatchError::ProcessWait(e, io::Error::last_os_error()));
        }
    };

    loop {
        // Attempt to read from the file descriptor
        let mut timeout = libc::timeval {
            tv_sec: Duration::from_secs(5).as_secs() as i64,
            tv_usec: 0,
        };
        let mut read_fds = unsafe { std::mem::zeroed::<libc::fd_set>() }; // Initialize fd_set
        unsafe { libc::FD_SET(pidfd, &mut read_fds) }; // Add the pidfd to the read set
        let result = unsafe { libc::select(pidfd + 1, &mut read_fds, ptr::null_mut(), ptr::null_mut(), &mut timeout) };

        if result < 0 {
            // An error occurred
            log::error!("Error occurred in select: {}", io::Error::last_os_error());
            break;
        } else if result == 0 {
            // Timeout occurred
            continue;
        } else {
            // At least one file descriptor is ready
            if unsafe { libc::FD_ISSET(pidfd, &read_fds) } {
                // The process has terminated
                log::info!("Process {} has terminated", pid);
                break;
            }
        }
        // Reset the read file descriptor set for the next select call
        unsafe { libc::FD_SET(pidfd, &mut read_fds) };
    }
    unsafe { libc::close(pidfd) };
    Ok(())
}

// Triggers one measurement (on all sources that support manual trigger).
fn trigger_measurement_now(pipeline: &MeasurementPipeline) -> anyhow::Result<()> {
    let control_handle = pipeline.control_handle();
    let send_task = control_handle.send_wait(
        request::source(SourceNamePattern::wildcard()).trigger_now(),
        TRIGGER_TIMEOUT,
    );
    pipeline
        .async_runtime()
        .block_on(send_task)
        .context("failed to send TriggerMessage")
}
