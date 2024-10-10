use std::process::{Command, Output, Stdio};

#[test]
fn help_local() {
    let status = cargo_run("alumet-local-agent", &["local_x86"], &["--help"]);
    assert!(status.success());
}

#[test]
fn help_relay_client() {
    let status = cargo_run("alumet-relay-client", &["relay_client"], &["--help"]);
    assert!(status.success());
}

#[test]
fn help_relay_server() {
    let status = cargo_run("alumet-relay-server", &["relay_server"], &["--help"]);
    assert!(status.success());
}

#[test]
fn client_bad_collector_uri() {
    let out = cargo_run_capture_output(
        "alumet-relay-client",
        &["relay_client"],
        &[
            "--plugins",
            "relay-client",
            "--collector-uri",
            "BADuri#é",
            "exec",
            "sleep 1",
        ],
    );
    assert!(
        !out.status.success(),
        "Alumet relay client should fail because of the bad collector-uri"
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    // println!("{stdout}");
    // println!("---------");
    // println!("{stderr}");
    let msg = "invalid uri BADuri#é";
    assert!(
        stderr.contains(msg) || stdout.contains(msg),
        "The incorrect URI should show up in the logs"
    );
}

fn cargo_run(binary: &str, features: &[&str], bin_args: &[&str]) -> std::process::ExitStatus {
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
    cmd.spawn()
        .unwrap_or_else(|_| panic!("could not spawn process: {cmd:?}"))
        .wait()
        .unwrap_or_else(|_| panic!("could not wait for process: {cmd:?}"))
}

fn cargo_run_capture_output(binary: &str, features: &[&str], bin_args: &[&str]) -> Output {
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

    let child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|_| panic!("could not spawn process: {cmd:?}"));

    child.wait_with_output().unwrap()
}
