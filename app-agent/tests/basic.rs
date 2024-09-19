use std::process::Command;

#[test]
fn help_local() {
    cargo_run("alumet-local-agent", &["local_x86"], &["--help"]);
}

#[test]
fn help_relay_client() {
    cargo_run("alumet-relay-client", &["relay_client"], &["--help"]);
}

#[test]
fn help_relay_server() {
    cargo_run("alumet-relay-server", &["relay_server"], &["--help"]);
}

fn cargo_run(binary: &str, features: &[&str], bin_args: &[&str]) {
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
        .unwrap_or_else(|_| panic!("could spawn process: {cmd:?}"))
        .wait()
        .unwrap_or_else(|_| panic!("could wait for process: {cmd:?}"));
    assert!(exit_status.success(), "command failed: {cmd:?}");
}
