//! Watch processes through it's pid
//! 
use mio::{unix::SourceFd, Events, Interest, Poll, Token, Waker};
use std::{fs::File, io::{ErrorKind, Read}, num::ParseIntError, os::fd::AsRawFd, process::ExitStatus, time::Duration};
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

const MOUNT_TOKEN: Token = Token(0);
const TIMER_TOKEN: Token = Token(1);
const STOP_TOKEN: Token = Token(2);
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
        .parse::<u32>()
        .map_err(|e| WatchError::PidCheck(pid.to_string(), e))?;

    if let Err(e) = trigger_measurement_now(&agent.pipeline) {
        log::error!("Could not trigger a first measurement before the child spawn: {e}");
    }

    // Spawn the process and wait for it to exit.
    let exit_status = wait_child(pid)?;
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
fn wait_child(pid: String) -> Result<ExitStatus, WatchError> {
    // According to `man proc_mounts`, a filesystem mount or unmount causes
    // `poll` and `epoll_wait` to mark the file as having a PRIORITY event.
    let path = format!("/proc/{pid}/mounts");
    let fd = File::open(&path).map_err( |e | WatchError::PidFilesCheck(pid.clone(), path, e))?;
    let binding = fd.as_raw_fd();
    let mut fd = SourceFd(&binding);
    
    // Prepare epoll.
    let mut poll = Poll::new().map_err(WatchError::PollInit)?;
    let stop_waker = Waker::new(poll.registry(), STOP_TOKEN).map_err(WatchError::PollInit)?;

    poll.registry()
        .register(&mut fd, MOUNT_TOKEN, Interest::READABLE | Interest::PRIORITY)
        .map_err(WatchError::PollInit)?;

    let mut events = Events::with_capacity(8); // we don't expect many events
    // let mut state = State::new(callback);

    loop {
        let poll_res = poll.poll(&mut events, Some(POLL_TIMEOUT));
        println!("^^^^^^^^^^^^^^^^^ poll res: {:?}\n", poll_res);
        if let Err(e) = poll_res {
            if e.kind() == ErrorKind::Interrupted {
                log::error!("Interrupted: continue");
                break; // retry
            } else {
                log::error!("propagate error");
                return Err(WatchError::PollPoll(e)); // propagate error
            }
        }

        // Call next() because we are not interested in each individual event.
        // If the timeout elapses, the event list is empty.
        // println!("Event is: {:?}", events);
        // if let Some(event) = events.iter().next() {
        //     log::debug!("event on /proc/{pid}/mounts: {event:?}");

        //     // the stop_waker has been triggered, which means that we must stop now
        //     if event.token() == STOP_TOKEN {
        //         log::error!("BREAK: Stop");
        //         break; // stop
        //     }
        // }
        for event in events.iter() {
            if event.token() == STOP_TOKEN {
                // Handle stop condition
                break; // stop
            }

            if event.token() == MOUNT_TOKEN {
                if event.is_readable() || event.is_priority() {
                    // Read the contents of the mounts file
                    println!("!!!!!!!!!!!!! Current mounts:\n");
                }
            }
        }
    }

    //Await, attendre... ?

    log::error!("WAITING");
    let status = 1;

    // // Spawn the process.
    // let mut p = Command::new(external_command.clone())
    //     .args(args)
    //     .spawn()
    //     .map_err(|e| WatchError::ProcessSpawn(external_command.clone(), e))?;

    // // Notify the plugins that there is a process to observe.
    // let pid = p.id();
    // log::info!("Child process '{external_command}' spawned with pid {pid}.");
    // crate::plugin::event::start_consumer_measurement()
    //     .publish(StartConsumerMeasurement(vec![ResourceConsumer::Process { pid }]));

    // // Wait for the process to terminate.
    // let status = p.wait().map_err(|e| WatchError::ProcessWait(pid, e))?;
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
