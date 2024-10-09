use std::{
    process::{Command, Output, Stdio},
    time::Duration,
};

#[test]
fn help_local() {
    cargo_run_fine("alumet-local-agent", &["local_x86"], &["--help"]);
}

#[test]
fn help_relay_client() {
    cargo_run_fine("alumet-relay-client", &["relay_client"], &["--help"]);
}

#[test]
fn help_relay_server() {
    cargo_run_fine("alumet-relay-server", &["relay_server"], &["--help"]);
}

#[test]
fn client_bad_collector_uri() {
    let out = cargo_run_timeout(
        "alumet-relay-client",
        &["relay_client"],
        &["--collector-uri", "BADuri#é"],
        Duration::from_secs(5),
    );
    assert!(
        !out.status.success(),
        "Alumet relay client should fail because of the bad collector-uri"
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    println!("{stdout}");
    println!("---------");
    println!("{stderr}");
    let msg = "invalid uri BADuri#é";
    assert!(
        stderr.contains(msg) || stdout.contains(msg),
        "The incorrect URI should show up in the logs"
    );
}

fn cargo_run_fine(binary: &str, features: &[&str], bin_args: &[&str]) {
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
    let exit_status = cmd
        .spawn()
        .unwrap_or_else(|_| panic!("could not spawn process: {cmd:?}"))
        .wait()
        .unwrap_or_else(|_| panic!("could not wait for process: {cmd:?}"));
    assert!(exit_status.success(), "command failed: {cmd:?}");
}

fn cargo_run_timeout(binary: &str, features: &[&str], bin_args: &[&str], timeout: Duration) -> Output {
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

    let thread = std::thread::spawn(move || {
        let mut child = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap_or_else(|_| panic!("could not spawn process: {cmd:?}"));

        const STEP: Duration = Duration::from_millis(100);
        let step: usize = STEP.as_millis().try_into().unwrap();
        let limit: u64 = timeout
            .as_millis()
            .try_into()
            .expect("number of ms should fit in 64bits");
        for _ in (0..=limit).step_by(step) {
            std::thread::sleep(STEP);
            // if let Some(_) = child.try_wait().unwrap() {
            //     return child.wait_with_output().unwrap();
            // }
            return child.wait_with_output().unwrap();
        }
        child
            .kill()
            .expect("should be able to kill process that takes too long to run");
        panic!("process timeout exceeded: {timeout:?}");
    });
    return thread.join().expect("thread should terminate fine");
}
