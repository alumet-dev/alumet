//! Integration tests for the "relay server" agent.
use crate::common::{empty_temp_dir, run::run_agent, tests};

#[test]
fn help() {
    let tmp_dir = empty_temp_dir("help").unwrap();
    let status = run_agent("alumet-relay-server", &["--help"], &tmp_dir).unwrap();
    assert!(status.success());
}

#[test]
fn args_bad_config_no_folder() -> anyhow::Result<()> {
    tests::args_bad_config_no_folder("alumet-relay-server")
}

#[test]
fn args_bad_config_missing_file_no_default() -> anyhow::Result<()> {
    tests::args_bad_config_missing_file_no_default("alumet-relay-server")
}

#[test]
fn args_regen_config() -> anyhow::Result<()> {
    tests::args_regen_config("alumet-relay-server")
}
