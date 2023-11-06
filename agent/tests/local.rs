//! Integration tests for the local agent.
mod common;

use anyhow::Context;

use common::{
    empty_temp_dir,
    run::{run_agent, run_agent_tee},
    tests,
};
use indoc::indoc;
use pretty_assertions::assert_eq;

const AGENT_BIN: &str = "alumet-agent";

#[test]
fn help() {
    let tmp_dir = empty_temp_dir("help").unwrap();
    let status = run_agent(AGENT_BIN, &["--help"], &tmp_dir).unwrap();
    assert!(status.success());
}

#[test]
fn args_bad_config_no_folder() -> anyhow::Result<()> {
    tests::args_bad_config_no_folder(AGENT_BIN)
}

#[test]
fn args_bad_config_missing_file_no_default() -> anyhow::Result<()> {
    tests::args_bad_config_missing_file_no_default(AGENT_BIN)
}

#[test]
fn args_regen_config() -> anyhow::Result<()> {
    tests::args_regen_config(AGENT_BIN)
}

#[test]
fn regen_config_with_plugins() -> anyhow::Result<()> {
    let tmp_dir = tempfile::tempdir()?;
    let conf = tmp_dir.path().join("config.toml");
    assert!(!conf.try_exists()?, "config file should not exist: {conf:?}");

    let conf_path_str = conf.to_str().unwrap();
    let output = run_agent_tee(
        AGENT_BIN,
        &["--plugins", "rapl", "--config", conf_path_str, "config", "regen"],
        tmp_dir.path(),
    )?;
    assert!(output.status.success(), "command should succeed");

    let config_content = std::fs::read_to_string(conf)?;
    let expected = indoc! { r#"
        [plugins.rapl]
        poll_interval = "1s"
        flush_interval = "5s"
        no_perf_events = false
    "# };
    assert_eq!(config_content, expected);
    Ok(())
}

#[test]
fn args_output_exec() -> anyhow::Result<()> {
    let tmp_dir = empty_temp_dir("args_output_exec").unwrap();
    let tmp_file_out = tmp_dir.join("agent-output.csv");
    let tmp_file_conf = tmp_dir.join("agent-config.toml");
    let _ = std::fs::create_dir(&tmp_dir);
    let _ = std::fs::remove_file(&tmp_file_out);
    let _ = std::fs::remove_file(&tmp_file_conf);

    // Check that the agent runs properly with --output
    let tmp_file_out_str = tmp_file_out.to_str().unwrap();
    let tmp_file_conf_str = tmp_file_conf.to_str().unwrap();
    let command_out = run_agent_tee(
        AGENT_BIN,
        &[
            "--output-file",
            tmp_file_out_str,
            "--config",
            tmp_file_conf_str,
            "--plugins",
            "procfs,csv",
            "exec",
            "sleep",
            "1",
        ],
        &tmp_dir,
    )?;
    assert!(
        command_out.status.success(),
        "alumet-agent --output-file FILE should work"
    );

    // Check that something has been written, at the right place.
    // If this fails, it may be (from most likely to less likely):
    // - the CLI args are not applied properly and the output was written elsewhere
    // - the CLI args 'config', 'plugins' or 'exec' are not applied properly
    // - there is a bug in the CSV plugin
    // - there is a bug in the core of Alumet or in the CSV plugin
    let alumet_out =
        std::fs::read_to_string(&tmp_file_out).with_context(|| format!("failed to read {tmp_file_out_str}"))?;
    assert!(alumet_out.contains("value"));
    Ok(())
}
