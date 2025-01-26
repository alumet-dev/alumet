//! Spawning child processes and watching them.

use std::{
    process::{Command, ExitStatus},
    time::Duration,
};

use anyhow::Context;

use crate::{
    pipeline::{control::ControlMessage, matching::TypedElementSelector, MeasurementPipeline},
    plugin::event::StartConsumerMeasurement,
    resources::ResourceConsumer,
};

use super::RunningAgent;
use thiserror::Error;

/// Error that can occur in [`watch_process`].
#[derive(Error, Debug)]
pub enum WatchError {
    /// The process could not be spawned.
    #[error("failed to spawn process {0}")]
    ProcessSpawn(String, #[source] std::io::Error),
    /// The process has spawned but waiting for it has failed.
    #[error("failed to wait for pid {0}")]
    ProcessWait(u32, #[source] std::io::Error),
    /// An error occured while waiting for the measurement pipeline to shut down.
    ///
    /// The error probably originated from inside a pipeline element (source, transform, output)
    /// and not from the shutdown operation.
    #[error("error in pipeline")]
    PipelineShutdown(#[source] anyhow::Error),
}

/// Spawns a process and stops the agent when the process exits.
///
/// The measurement sources are triggered before the process spawns and after it exits.
pub fn watch_process(
    agent: RunningAgent,
    program: String,
    args: Vec<String>,
    shutdown_timeout: Duration,
) -> Result<(), WatchError> {
    // At least one measurement.
    if let Err(e) = trigger_measurement_now(&agent.pipeline) {
        log::error!("Could not trigger a first measurement before the child spawn: {e}");
    }

    // Spawn the process and wait for it to exit.
    let exit_status = exec_child(program, args)?;
    log::info!("Child process exited with status {exit_status}, Alumet will now stop.");

    // One last measurement.
    if let Err(e) = trigger_measurement_now(&agent.pipeline) {
        log::error!("Could not trigger one last measurement after the child exit: {e}");
    }

    // Stop the pipeline
    agent.pipeline.control_handle().shutdown();
    agent
        .wait_for_shutdown(shutdown_timeout)
        .map_err(WatchError::PipelineShutdown)
}

/// Spawns a child process and waits for it to exit.
fn exec_child(external_command: String, args: Vec<String>) -> Result<ExitStatus, WatchError> {
    // Spawn the process.
    let mut p = Command::new(external_command.clone())
        .args(args)
        .spawn()
        .map_err(|e| WatchError::ProcessSpawn(external_command.clone(), e))?;

    // Notify the plugins that there is a process to observe.
    let pid = p.id();
    log::info!("Child process '{external_command}' spawned with pid {pid}.");
    crate::plugin::event::start_consumer_measurement()
        .publish(StartConsumerMeasurement(vec![ResourceConsumer::Process { pid }]));

    // Wait for the process to terminate.
    let status = p.wait().map_err(|e| WatchError::ProcessWait(pid, e))?;
    Ok(status)
}

// Triggers one measurement (on all sources that support manual trigger).
fn trigger_measurement_now(pipeline: &MeasurementPipeline) -> anyhow::Result<()> {
    use crate::pipeline::elements::source;

    let control_handle = pipeline.control_handle();
    let send_task = control_handle.send(ControlMessage::Source(source::ControlMessage::TriggerManually(
        source::TriggerMessage {
            selector: TypedElementSelector::all(),
        },
    )));
    pipeline
        .async_runtime()
        .block_on(send_task)
        .context("failed to send TriggerMessage")
}
