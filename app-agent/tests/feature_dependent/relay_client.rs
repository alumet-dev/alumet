//! Integration tests for the "relay client" agent.
use crate::common::{
    empty_temp_dir,
    run::{run_agent, run_agent_tee},
    tests,
};

#[test]
fn help() {
    let tmp_dir = empty_temp_dir("help").unwrap();
    let status = run_agent("alumet-relay-client", &["--help"], &tmp_dir).unwrap();
    assert!(status.success());
}

#[test]
fn args_bad_relay_server_address() {
    let tmp_dir = empty_temp_dir("args_bad_relay_server_address").unwrap();
    let out = run_agent_tee(
        "alumet-relay-client",
        &[
            "--plugins",
            "relay-client",
            "--relay-server",
            "BADuri:#é",
            "exec",
            "sleep 1",
        ],
        &tmp_dir,
    )
    .expect("failed to run the client and capture its output");
    assert!(
        !out.status.success(),
        "Alumet relay client should fail because of the bad relay server address"
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    let msg = "BADuri:#é";
    assert!(
        stderr.contains(msg) || stdout.contains(msg),
        "The incorrect URI should show up in the logs"
    );
}

#[test]
fn args_bad_config_no_folder() -> anyhow::Result<()> {
    tests::args_bad_config_no_folder("alumet-relay-client")
}

#[test]
fn args_bad_config_missing_file_no_default() -> anyhow::Result<()> {
    tests::args_bad_config_missing_file_no_default("alumet-relay-client")
}

#[test]
fn args_regen_config() -> anyhow::Result<()> {
    tests::args_regen_config("alumet-relay-client")
}
