//! Tie Alumet to a running process.

use std::{
    fs, os::unix::fs::PermissionsExt, process::ExitStatus
};

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
    let p_result = std::process::Command::new(external_command.clone())
        .args(args.clone())
        .spawn();

    let mut p = match p_result {
        Ok(val) => val,
        Err(e) => match e.kind() {
            std::io::ErrorKind::NotFound => {
                let directory_entries_iter = fs::read_dir("./").unwrap();
                let mut entry_list = String::from("Available files in the current directory:\n");
                for entry_result in directory_entries_iter {
                    let entry = entry_result.unwrap();
                    let entry_type = entry.file_type().unwrap();
                    if entry_type.is_file() {
                        let entry_string = entry.file_name().into_string().unwrap();
                        if external_command == entry_string {
                            if let Ok(canonical_path) = fs::canonicalize(entry.path()) {
                                if let Some(parent_path) = canonical_path.parent() {
                                    log::info!("Found corresponding file in the current directory: {:?}", parent_path);
                                } else {
                                    log::info!("Found corresponding file in the current directory.");
                                }
                            } else {
                                log::info!("Found corresponding file in the current directory.");
                            }
                            // Check for execution permissions
                            let mut checkup_message = String::from(format!("Who can execute the file: {}:\n", external_command));
                            if entry.metadata().unwrap().permissions().mode() & 0o100 != 0 {
                                checkup_message = format!("{} - User: YES\n", checkup_message);
                            } else {
                                checkup_message = format!("{} - User: NO\n", checkup_message);
                            }
                            if entry.metadata().unwrap().permissions().mode() & 0o010 != 0 {
                                checkup_message = format!("{} - Group: YES\n", checkup_message);
                            } else {
                                checkup_message = format!("{} - Group: NO\n", checkup_message);
                            }
                            if entry.metadata().unwrap().permissions().mode() & 0o001 != 0 {
                                checkup_message = format!("{} - Others: YES\n", checkup_message);
                            } else {
                                checkup_message = format!("{} - Others: NO\n", checkup_message);
                            }
                            if !external_command.starts_with("./") {
                                checkup_message = format!("{}\n Please try again using the following syntax:\n", checkup_message);
                                checkup_message = format!("{} [...] exec ./{}", checkup_message, external_command);
                                for argument in &args {
                                    checkup_message = format!("{} {}", checkup_message, argument)
                                }
                            }
                            log::error!("{}",checkup_message);
                            panic!("Maybe you could change the path to match with the correct one")
                        } else {
                            // Unable to find a corresponding things to execute
                            entry_list = format!("{} -{}\n", entry_list, entry_string)
                        }
                    }
                }
                log::error!("Executable not found in the current directory, found:\n{}", entry_list);
                panic!("Maybe you could change the path")
            }
            _ => {
                panic!("Error in child process");
            }
        },
    };

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
