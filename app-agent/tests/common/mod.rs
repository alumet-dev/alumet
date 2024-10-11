use std::{
    io::{Read, Write},
    process::{Command, Output, Stdio},
    thread,
};

/// Constructs a `Command` that will do `cargo run --bin ${binary} [--features ${features}] [-- ${bin_args}]`
fn command_cargo_run(binary: &str, features: &[&str], bin_args: &[&str]) -> Command {
    let mut cmd = Command::new("cargo");
    cmd.args(["run", "--bin", binary]);
    if !features.is_empty() {
        cmd.arg("--features");
        cmd.args(features);
    }
    if !bin_args.is_empty() {
        cmd.arg("--");
        cmd.args(bin_args);
    }
    cmd
}

/// Executes `cargo run ...` and returns its exit status.
///
/// The stdout and stderr are inherited from the current process.
pub fn cargo_run(binary: &str, features: &[&str], bin_args: &[&str]) -> std::process::ExitStatus {
    let mut cmd = command_cargo_run(binary, features, bin_args);
    cmd.spawn()
        .unwrap_or_else(|_| panic!("could not spawn process: {cmd:?}"))
        .wait()
        .unwrap_or_else(|_| panic!("could not wait for process: {cmd:?}"))
}

/// Executes `cargo run ...` and captures its output.
///
/// The stdout and stderr are captured, nothing will be printed during the execution of the command.
pub fn cargo_run_capture_output(binary: &str, features: &[&str], bin_args: &[&str]) -> Output {
    let mut cmd = command_cargo_run(binary, features, bin_args);
    let child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|_| panic!("could not spawn process: {cmd:?}"));

    child.wait_with_output().unwrap()
}

/// Executes `cargo run ...` and duplicates its output to the current stdout/stderr and two buffers.
///
/// The stdout and stderr are both redirected to a pipe, and copied to the current stdout and stderr,
/// and to two buffers. In essence, it does "both" `cargo_run` and `cargo_run_capture_output`.
pub fn cargo_run_tee(binary: &str, features: &[&str], bin_args: &[&str]) -> anyhow::Result<Output> {
    let mut cmd = command_cargo_run(binary, features, bin_args);
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|_| panic!("could not spawn process: {cmd:?}"));

    let child_stdout = child.stdout.take().expect("could not attach to child stdout");
    let child_stderr = child.stderr.take().expect("could not attach to child stderr");

    fn tee(mut stream: impl Read, a: &mut impl Write, b: &mut impl Write) -> std::io::Result<()> {
        let mut buf = [0u8; 256];
        loop {
            // read from input
            let n_read = stream.read(&mut buf)?;
            if n_read == 0 {
                break;
            }

            // write to all outputs
            let buf = &buf[..n_read];
            a.write_all(buf)?;
            b.write_all(buf)?;
        }
        Ok(())
    }

    let mut stdout_buf = Vec::with_capacity(512);
    let mut stderr_buf = Vec::with_capacity(512);
    let stdout_thread = thread::spawn(move || {
        tee(child_stdout, &mut stdout_buf, &mut std::io::stdout().lock())?;
        anyhow::Ok(stdout_buf)
    });
    let stderr_thread = thread::spawn(move || {
        tee(child_stderr, &mut stderr_buf, &mut std::io::stderr().lock())?;
        anyhow::Ok(stderr_buf)
    });

    let stdout = stdout_thread.join().unwrap()?;
    let stderr = stderr_thread.join().unwrap()?;

    let status = child.wait()?;
    Ok(Output { status, stdout, stderr })
}
