use anyhow::Context;
use assert_cmd::cargo::{CargoError, CommandCargoExt};
use std::{
    io::{Read, Write},
    ops::{Deref, DerefMut},
    path::Path,
    process::{Child, Command, Output, Stdio},
    thread,
};

/// Constructs a `Command` that execute a binary.
///
/// This does NOT call `cargo run`, see [`assert_cmd::Command::cargo_bin`].
pub fn command_run_agent(binary: &str, bin_args: &[&str]) -> Result<Command, CargoError> {
    let mut cmd = Command::cargo_bin(binary)?;
    cmd.args(bin_args);
    Ok(cmd)
}

/// Executes an agent binary with the given arguments, and returns its exit status.
///
/// The stdout and stderr are inherited from the current process.
pub fn run_agent(binary: &str, bin_args: &[&str], workdir: &Path) -> anyhow::Result<std::process::ExitStatus> {
    let mut cmd = command_run_agent(binary, bin_args)?;
    cmd.current_dir(workdir)
        .spawn()
        .with_context(|| format!("could not spawn process {cmd:?}"))?
        .wait()
        .with_context(|| format!("could not wait for process: {cmd:?}"))
}

/// Executes `cargo run <binary> <bin_args>` in `workdir` and
/// duplicates its output to the current stdout/stderr and two buffers.
///
/// The stdout and stderr are both redirected to a pipe, and copied to the current stdout and stderr,
/// and to two buffers. The buffers are returned in an [`Output`].
pub fn run_agent_tee(binary: &str, bin_args: &[&str], workdir: &Path) -> anyhow::Result<Output> {
    let mut cmd = command_run_agent(binary, bin_args)?;
    let child = cmd
        .current_dir(workdir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("could not spawn process: {cmd:?}: {e}"));
    let mut child = ChildGuard::new(child);

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

/// A wrapper around a child process that kills the child on drop.
pub struct ChildGuard(Option<Child>);

impl ChildGuard {
    pub fn new(process: Child) -> Self {
        Self(Some(process))
    }

    pub fn take(&mut self) -> Child {
        self.0.take().unwrap()
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            if let Err(e) = child.kill() {
                println!("ERROR: failed to kill child {} on drop: {e}", child.id());
            }
        }
    }
}

impl Deref for ChildGuard {
    type Target = Child;

    fn deref(&self) -> &Self::Target {
        self.0.as_ref().unwrap()
    }
}

impl DerefMut for ChildGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.as_mut().unwrap()
    }
}
