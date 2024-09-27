//! Tie Alumet to a running process.

use std::process::ExitStatus;

use alumet::{
    pipeline::{
        control::ControlMessage,
        elements::source::{self, TriggerMessage},
        matching::TypedElementSelector,
        MeasurementPipeline,
    },
    plugin::event::StartConsumerMeasurement,
    resources::ResourceConsumer,
};
use anyhow::Context;

/// Spawns a child process and waits for it to exit.
pub fn exec_child(external_command: String, args: Vec<String>) -> anyhow::Result<ExitStatus> {
    // Spawn the process.
    let mut p = std::process::Command::new(external_command.clone())
        .args(args)
        .spawn()
        .expect("error in child process");

    // Notify the plugins that there is a process to observe.
    let pid = p.id();
    log::info!("Child process '{external_command}' spawned with pid {pid}.");
    alumet::plugin::event::start_consumer_measurement()
        .publish(StartConsumerMeasurement(vec![ResourceConsumer::Process { pid }]));

    // Wait for the process to terminate.
    let status = p.wait().context("failed to wait for child process")?;
    Ok(status)
}

// Triggers one measurement (on all sources that support manual trigger).
pub fn trigger_measurement_now(pipeline: &MeasurementPipeline) -> anyhow::Result<()> {
    let control_handle = pipeline.control_handle();
    let send_task = control_handle.send(ControlMessage::Source(source::ControlMessage::TriggerManually(
        TriggerMessage {
            selector: TypedElementSelector::all(),
        },
    )));
    pipeline
        .async_runtime()
        .block_on(send_task)
        .context("failed to send TriggerMessage")
}
