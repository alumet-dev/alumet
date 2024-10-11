use anyhow::Context;
use common::{cargo_run, cargo_run_tee};

mod common;

#[test]
fn help() {
    let status = cargo_run("alumet-local-agent", &["local_x86"], &["--help"]);
    assert!(status.success());
}

#[test]
fn args_output_exec() -> anyhow::Result<()> {
    let tmp_dir = std::env::temp_dir().join(format!("{}-args_output_exec", env!("CARGO_CRATE_NAME")));
    let tmp_file_out = tmp_dir.join("agent-output.csv");
    let tmp_file_conf = tmp_dir.join("agent-config.toml");
    let _ = std::fs::create_dir(&tmp_dir);
    let _ = std::fs::remove_file(&tmp_file_out);
    let _ = std::fs::remove_file(&tmp_file_conf);

    // Check that the agent runs properly with --output
    let tmp_file_out_str = tmp_file_out.to_str().unwrap();
    let tmp_file_conf_str = tmp_file_conf.to_str().unwrap();
    let command_out = cargo_run_tee(
        "alumet-local-agent",
        &["local_x86"],
        &[
            "--output",
            tmp_file_out_str,
            "--config",
            tmp_file_conf_str,
            "--plugins",
            "procfs,csv",
            "exec",
            "sleep",
            "1",
        ],
    )?;
    assert!(command_out.status.success());

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
