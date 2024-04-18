use std::{
    io::Read,
    os::unix::net::UnixListener,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use alumet::pipeline::{
    runtime::{BlockingControlHandle, ControlHandle, OutputCmd, SourceCmd, TransformCmd},
    trigger::Trigger,
};
use anyhow::{anyhow, Context};

pub struct SocketControl {
    socket_path: String,
    running: Arc<AtomicBool>,
    thread_handle: JoinHandle<()>,
}

impl SocketControl {
    pub fn start_new(handle: ControlHandle) -> anyhow::Result<SocketControl> {
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

        let running = Arc::new(AtomicBool::new(true));
        let thread_flag = running.clone();
        let thread_handle = std::thread::spawn(move || {
            while thread_flag.load(Ordering::Relaxed) {
                match accept_and_handle(&listener, &handle) {
                    Ok(_) => log::debug!("Command sent successfully."),
                    Err(e) => log::error!("Command failed: {e}"),
                };
            }
        });
        Ok(SocketControl {
            socket_path: socket_path.to_owned(),
            running,
            thread_handle,
        })
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
    }

    pub fn join(self) {
        self.thread_handle.join().expect("failed to join thread");
        let _ = std::fs::remove_file(self.socket_path);
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
fn run_command(command: String, handle: &ControlHandle) -> anyhow::Result<()> {
    fn parse_source_command(args: &[&str]) -> anyhow::Result<SourceCmd> {
        match args {
            ["pause"] => Ok(SourceCmd::Pause),
            ["run"] => Ok(SourceCmd::Run),
            ["stop"] => Ok(SourceCmd::Stop),
            ["trigger", "every", interval_str] => {
                let poll_interval = parse_duration(&interval_str)?;
                let flush_interval = poll_interval;
                Ok(SourceCmd::SetTrigger(Some(Trigger::TimeInterval {
                    start_time: Instant::now(),
                    poll_interval,
                    flush_interval,
                })))
            }
            _ => Err(anyhow!("invalid arguments for source command: {args:?}")),
        }
    }

    fn parse_transform_command(args: &[&str]) -> anyhow::Result<TransformCmd> {
        match args {
            ["enable"] => Ok(TransformCmd::Enable),
            ["disable"] => Ok(TransformCmd::Disable),
            _ => Err(anyhow!("invalid arguments for transform command: {args:?}")),
        }
    }

    fn parse_output_command(args: &[&str]) -> anyhow::Result<OutputCmd> {
        match args {
            ["pause"] => Ok(OutputCmd::Pause),
            ["run"] => Ok(OutputCmd::Run),
            ["stop"] => Ok(OutputCmd::Stop),
            _ => Err(anyhow!("invalid arguments for output command: {args:?}")),
        }
    }

    fn run_blocking_command(handle: BlockingControlHandle, element: &str, args: &[&str]) -> anyhow::Result<()> {
        match element {
            "source" | "sources" => handle.control_sources(parse_source_command(args)?),
            "transform" | "transforms" => handle.control_transforms(parse_transform_command(args)?),
            "output" | "outputs" => handle.control_outputs(parse_output_command(args)?),
            _ => {
                return Err(anyhow!(
                    "invalid element \"{element}\", it should be source, transform or output"
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

/// Minimal duration parsing. Accepts inputs like `"2min"`, `"5s"`, `"5.17s"` and `"100ms"`.
fn parse_duration(d: &str) -> anyhow::Result<Duration> {
    fn parse_f64(number: &str) -> anyhow::Result<f64> {
        number.parse().with_context(|| format!("invalid float \"{number}\""))
    }
    fn parse_u64(number: &str) -> anyhow::Result<u64> {
        number.parse().with_context(|| format!("invalid integer \"{number}\""))
    }

    let is_number_char = |c: char| c.is_ascii_digit() || c == '.';
    let split_i = d
        .find(|c| !is_number_char(c))
        .with_context(|| format!("invalid duration \"{d}\", try something like \"5.2s\" or \"100ms\""))?;

    let (number, unit) = d.split_at(split_i);
    match unit {
        "s" | "sec" | "seconds" => {
            let secs = parse_f64(number)?;
            Ok(Duration::try_from_secs_f64(secs)?)
        }
        "ms" | "millis" => {
            let ms = parse_u64(number)?;
            Ok(Duration::from_millis(ms))
        }
        "mn" | "min" | "minutes" => {
            let min = parse_u64(number)?;
            let secs = min
                .checked_mul(60)
                .with_context(|| format!("{min} minutes is too big"))?;
            Ok(Duration::from_secs(secs))
        }
        _ => Err(anyhow!(
            "Invalid duration unit \"{unit}\", try something like \"5.2s\" or \"100ms\"."
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::socket::parse_duration;

    #[test]
    fn duration_parsing() {
        assert_eq!(parse_duration("5s").unwrap(), Duration::from_secs(5));
        assert_eq!(parse_duration("5.782s").unwrap(), Duration::from_millis(5782));
        assert_eq!(parse_duration("0.1s").unwrap(), Duration::from_millis(100));
        assert_eq!(parse_duration("5ms").unwrap(), Duration::from_millis(5));
        assert_eq!(parse_duration("5mn").unwrap(), Duration::from_secs(60 * 5));
        
        assert!(parse_duration("5").is_err());
        assert!(parse_duration("1abcd").is_err());
        assert!(parse_duration("sec").is_err());
        assert!(parse_duration("100000000000000000000000min").is_err());
    }
}
