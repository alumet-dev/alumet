//! Integration tests for the "relay client" agent.
use common::run::{cargo_run, cargo_run_tee};

mod common;

#[test]
fn help() {
    let status = cargo_run("alumet-relay-client", &["relay_client"], &["--help"]);
    assert!(status.success());
}

#[test]
fn args_bad_collector_uri() {
    let out = cargo_run_tee(
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
    )
    .expect("failed to run the client and capture its output");
    assert!(
        !out.status.success(),
        "Alumet relay client should fail because of the bad collector-uri"
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    let msg = "invalid uri BADuri#é";
    assert!(
        stderr.contains(msg) || stdout.contains(msg),
        "The incorrect URI should show up in the logs"
    );
}

#[test]
fn args_bad_config_no_folder() -> anyhow::Result<()> {
    common::tests::args_bad_config_no_folder("alumet-relay-client", &["relay_client"])
}

#[test]
fn args_bad_config_missing_file_no_default() -> anyhow::Result<()> {
    common::tests::args_bad_config_missing_file_no_default("alumet-relay-client", &["relay_client"])
}

#[test]
fn args_regen_config() -> anyhow::Result<()> {
    common::tests::args_regen_config("alumet-relay-client", &["relay_client"])
}
