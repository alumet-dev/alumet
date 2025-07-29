//! Watch processes through it's pid
use std::{fs::{self, File}, io::{self, ErrorKind, Read}, num::ParseIntError, os::fd::AsRawFd, path::PathBuf, process::ExitStatus, ptr, thread, time::Duration};
use nc::pid_t;
use thiserror::Error;

use anyhow::Context;

use crate::pipeline::{control::request, matching::SourceNamePattern, MeasurementPipeline};

use super::{builder::ShutdownError, RunningAgent};

/// Error that can occur in [`watch_process`].
#[derive(Error, Debug)]
pub enum WatchError {
    /// The process could not be spawned.
    #[error("failed to check process with pid {0}")]
    PidCheck(String, #[source] std::num::ParseIntError),
    /// The process's files could not be reached.
    #[error("failed to check reach file: {1} for pid {0}")]
    PidFilesCheck(String, String, #[source] std::io::Error),
    /// The process has spawned but waiting for it has failed.
    #[error("failed to wait for pid {0}")]
    ProcessWait(u32, #[source] std::io::Error),
    /// An error occurred while waiting for the agent to shut down.
    #[error("error in shutdown")]
    Shutdown(#[source] ShutdownError),
     // #[error("read error")]
    // MountRead(#[from] ReadError),
    #[error("failed to initialize epoll")]
    PollInit(#[source] std::io::Error),
    #[error("poll.poll() returned an error")]
    PollPoll(#[source] std::io::Error),
    #[error("failed to register a timer to epoll")]
    PollTimer(#[source] std::io::Error),
    #[error("could not set up a timer with delay {0:?} for event coalescing")]
    Timerfd(Duration, #[source] std::io::Error),
    #[error("failed to stop epoll from another thread")]
    Stop(#[source] std::io::Error),
}

const POLL_TIMEOUT: Duration = Duration::from_secs(5);
const PROC_MOUNTS_PATH: &str = "/proc/mounts";
const TRIGGER_TIMEOUT: Duration = Duration::from_secs(1);


/// Watch process that runs identified by it's pid until it's end.
///
/// The measurement sources are triggered before the process spawns and after it exits.
///
/// After the process exits, the pipeline must stop within `shutdown_timeout`, or an error is returned.
pub fn watch_process(
    agent: RunningAgent,
    pid: String,
    shutdown_timeout: Duration,
) -> Result<(), WatchError> {
    println!("THIS IS WATCH COMMAND");
    log::error!("THIS IS WATCH COMMAND - {pid}");

    // Check if we can convert the pid to u332
    let pid_u32 = pid
        .parse::<i32>()
        .map_err(|e| WatchError::PidCheck(pid.to_string(), e))?;

    if let Err(e) = trigger_measurement_now(&agent.pipeline) {
        log::error!("Could not trigger a first measurement before the child spawn: {e}");
    }

    // Spawn the process and wait for it to exit.
    let exit_status = wait_child(pid_u32)?;
    log::info!("Child process exited with status {exit_status}, Alumet will now stop.");

    // One last measurement.
    if let Err(e) = trigger_measurement_now(&agent.pipeline) {
        log::error!("Could not trigger one last measurement after the child exit: {e}");
    }

    // Stop the pipeline
    agent.pipeline.control_handle().shutdown();
    agent.wait_for_shutdown(shutdown_timeout).map_err(WatchError::Shutdown)
}

/// Spawns a child process and waits for it to exit.
fn wait_child(pid: i32) -> Result<ExitStatus, WatchError> {
    let pidfd = unsafe { nc::pidfd_open(pid, 0) };
    if pidfd == Err(nc::errno::ENOSYS) {
        log::warn!("PIDFD_OPEN syscall not supported in this system use of another way");
        return Ok(ExitStatus::default());
    }
    let pidfd = pidfd.unwrap();
    
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


    log::error!("WAITING");
    let status = 1;

    Ok(ExitStatus::default())
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
