use std::{
    io::Read,
    os::unix::net::UnixListener,
    time::{Duration, Instant},
};

use alumet::pipeline::{
    runtime::{BlockingControlHandle, ControlHandle, OutputCmd, SourceCmd, TransformCmd},
    trigger::TriggerProvider,
};
use anyhow::{anyhow, Context};

pub struct SocketControl;

impl SocketControl {
    pub fn start_new(handle: ControlHandle) -> anyhow::Result<()> {
        let socket_path = "alumet.sock";
        let _ = std::fs::remove_file(&socket_path);
        let listener = UnixListener::bind(socket_path)?;

        fn accept_and_handle(listener: &UnixListener, handle: &ControlHandle) -> anyhow::Result<()> {
            let (mut unix_stream, socket_addr) = listener.accept().expect("UnixListener::accept failed");

            let mut buf = String::new();
            if unix_stream.read_to_string(&mut buf).is_ok() {
                log::info!("Received command from {socket_addr:?}: {buf}");
            }
            if let Err(e) = run_command(buf, &handle) {
                log::error!("Command error: {e}");
            };
            Ok(())
        }

        let thread_handle = std::thread::spawn(move || loop {
            match accept_and_handle(&listener, &handle) {
                Ok(_) => log::debug!("Command sent successfully."),
                Err(e) => log::error!("Command failed: {e}"),
            };
        });
        Ok(())
    }
}

/// Parses a command from a string and executes it on the pipeline thanks to the ControlHandle.
///
/// ## Command Examples
/// ```sh
/// rapl:sources pause
/// rapl:sources run
/// rapl:sources trigger every 5s
/// outputs pause
/// ```
///
/// ## Command Syntax
///
/// ```bnf
/// [plugin:]element command [options...]
/// ```
///
fn run_command(command: String, handle: &ControlHandle) -> anyhow::Result<()> {
    fn parse_source_command(args: &[&str]) -> anyhow::Result<SourceCmd> {
        match args {
            ["pause"] => Ok(SourceCmd::Pause),
            ["run"] => Ok(SourceCmd::Run),
            ["stop"] => Ok(SourceCmd::Stop),
            ["trigger", "every", interval_str] => {
                let poll_interval = parse_duration(&interval_str)?;
                let flush_interval = poll_interval;
                Ok(SourceCmd::SetTrigger(Some(TriggerProvider::TimeInterval {
                    start_time: Instant::now(),
                    poll_interval,
                    flush_interval,
                })))
            }
            _ => Err(anyhow!("invalid arguments for source command {args:?}")),
        }
    }

    fn parse_transform_command(args: &[&str]) -> anyhow::Result<TransformCmd> {
        match args {
            ["enable"] => Ok(TransformCmd::Enable),
            ["disable"] => Ok(TransformCmd::Disable),
            _ => Err(anyhow!("invalid arguments for transform command {args:?}")),
        }
    }

    fn parse_output_command(args: &[&str]) -> anyhow::Result<OutputCmd> {
        match args {
            ["pause"] => Ok(OutputCmd::Pause),
            ["run"] => Ok(OutputCmd::Run),
            ["stop"] => Ok(OutputCmd::Stop),
            _ => Err(anyhow!("invalid arguments for output command {args:?}")),
        }
    }

    fn run_blocking_command(handle: BlockingControlHandle, element: &str, args: &[&str]) -> anyhow::Result<()> {
        match element {
            "source" | "sources" => handle.control_sources(parse_source_command(args)?),
            "transform" | "transforms" => handle.control_transforms(parse_transform_command(args)?),
            "output" | "outputs" => handle.control_outputs(parse_output_command(args)?),
            _ => {
                return Err(anyhow!(
                    "invalid element {element}, it should be source, transform or output"
                ))
            }
        }
        Ok(())
    }

    let parts: Vec<&str> = command.trim().split(' ').map(|s| s.trim()).collect();
    let scope: Vec<&str> = parts.get(0).context("missing scope")?.split(':').collect();
    let args: &[&str] = &parts[1..];
    match scope[..] {
        [plugin_name, element] => {
            let handle = handle.blocking_plugin(plugin_name);
            run_blocking_command(handle, element, args)?;
        }
        [element] => {
            let handle = handle.blocking_all();
            run_blocking_command(handle, element, args)?;
        }
        _ => {
            return Err(anyhow!(
                "invalid scope {}, expected something like plugin_name:element or element",
                parts[0]
            ))
        }
    };
    Ok(())
}

fn parse_duration(d: &str) -> anyhow::Result<Duration> {
    if let Some(secs) = d.strip_suffix("s") {
        Ok(Duration::try_from_secs_f32(secs.parse()?)?)
    } else if let Some(ms) = d.strip_suffix("ms").or_else(|| d.strip_suffix("millis")) {
        Ok(Duration::from_millis(ms.parse()?))
    } else if let Some(min) = d.strip_suffix("min").or_else(|| d.strip_suffix("mn")) {
        let minutes: u64 = min.parse()?;
        Ok(Duration::from_secs(minutes * 60))
    } else {
        Err(anyhow!("Invalid duration: {d}"))
    }
}
