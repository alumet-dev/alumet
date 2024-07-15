//! Tie Alumet to a running process.

use std::{fs::{self, File}, os::unix::fs::PermissionsExt, path::PathBuf, process::ExitStatus};

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
                let return_error: String = handle_not_found(external_command, args);
                panic!("{}", return_error);
            },
            std::io::ErrorKind::PermissionDenied => {
                let return_error: String = handle_permission_denied(external_command);
                panic!("{}", return_error);
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

fn handle_permission_denied(external_command: String) -> String {
    let file_permission_denied = match File::open(external_command.clone()) {
        Ok(file) => {
            file
        },
        Err(err) => {
            panic!("Error when try to read the file: {}", err);
        },
    };
    let metadata_file = file_permission_denied.metadata().expect(format!("Unable to retrieve metadata for: {}", external_command).as_str());
    // Check for user permissions.
    let user_perm = match metadata_file.permissions().mode() & 0o500 {
        0 => "rx",
        1 => "r",
        4 => "x",
        _ => "",
    };
    // Check for group permissions.
    let group_perm: &str = match metadata_file.permissions().mode() & 0o050 {
        0 => "rx",
        1 => "r",
        4 => "x",
        _ => "",
    };
    // Check for other permissions.
    let other_perm = match metadata_file.permissions().mode() & 0o005 {
        0 => "rx",
        1 => "r",
        4 => "x",
        _ => "",
    };
    if user_perm == "rx" || group_perm == "rx" || other_perm == "rx" {
        log::error!("file '{}' is missing the following permissions:  'rx'", external_command);
        log::info!("💡 Hint: try 'chmod +rx {}", external_command)
    } else if user_perm == "r" || group_perm == "r" || other_perm == "r" {
        log::error!("file '{}' is missing the following permissions:  'r'", external_command);
        log::info!("💡 Hint: try 'chmod +r {}", external_command)
    } else if user_perm == "x" || group_perm == "x" || other_perm == "x" {
        log::error!("file '{}' is missing the following permissions:  'x'", external_command);
        log::info!("💡 Hint: try 'chmod +x {}", external_command)
    } else {
        log::warn!("Can't determine right issue about the file: {}", external_command);
    }
    "Issue happened about file's permission".to_string()
}

fn handle_not_found(external_command: String, args: Vec<String>) -> String {
    fn resolve_application_path() -> std::io::Result<PathBuf> {
        std::env::current_exe()?.canonicalize()
    }
    log::error!("Command '{}' not found", external_command);
    let directory_entries_iter = match fs::read_dir(".") {
        Ok(directory) => {
            directory
        },
        Err(err) => {
            panic!("Error when try to read current directory: {}", err);
        },
    };
    let app_path = resolve_application_path()
                .ok()
                .and_then(|p| p.to_str().map(|s| s.to_owned()))
                .unwrap_or(String::from("path/to/agent"));
    for entry_result in directory_entries_iter {
        let entry = entry_result.unwrap();
        let entry_type = entry.file_type().unwrap();
        if entry_type.is_file() {
            let entry_string = entry.file_name().into_string().unwrap();
            if external_command == entry_string {
                log::info!("💡Hint: A file named '{}' exists in the current directory. Prepend ./ to execute it.", entry_string);
                log::info!("Example: {} exec ./{} {}",app_path, entry_string, args.join(" "));
            }
        }
    }

    "Issue happened because the file was not found".to_string()
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
