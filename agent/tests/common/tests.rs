use anyhow::Context;

use crate::common::{empty_temp_dir, run::run_agent_tee};

pub fn args_bad_config_no_folder(binary: &str) -> anyhow::Result<()> {
    let tmp_dir = empty_temp_dir(&format!("{binary}-args_bad_config_no_folder"))?;
    let bad_conf = tmp_dir.join("zzzzz.toml");

    let bad_conf_filename = bad_conf.to_str().unwrap();
    let output = run_agent_tee(binary, &["--config", bad_conf_filename], &tmp_dir)?;
    assert!(
        !output.status.success(),
        "should fail because the config directory does not exist"
    );
    let stdout = String::from_utf8(output.stdout)?;
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stdout.contains(bad_conf_filename) || stderr.contains(bad_conf_filename));
    Ok(())
}

pub fn args_bad_config_missing_file_no_default(binary: &str) -> anyhow::Result<()> {
    let tmp_dir = empty_temp_dir(&format!("{binary}-args_bad_config_missing_file_no_default"))?;
    let bad_conf = tmp_dir.join("zzzzz.toml");

    let bad_conf_filename = bad_conf.to_str().unwrap();
    let output = run_agent_tee(
        binary,
        &["--config", bad_conf_filename, "--no-default-config"],
        &tmp_dir,
    )?;
    assert!(
        !output.status.success(),
        "should fail because the config does not exist"
    );
    let stdout = String::from_utf8(output.stdout)?;
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stdout.contains(bad_conf_filename) || stderr.contains(bad_conf_filename));
    Ok(())
}

pub fn args_regen_config(binary: &str) -> anyhow::Result<()> {
    let tmp_dir = empty_temp_dir(&format!("{binary}-args_regen_config"))?;
    let conf = tmp_dir.join("config.toml");
    assert!(!conf.try_exists()?, "config file should not exist: {conf:?}");

    let conf_path_str = conf.to_str().unwrap();
    let output = run_agent_tee(binary, &["--config", conf_path_str, "regen-config"], &tmp_dir)?;
    assert!(output.status.success(), "command should succeed");

    let conf_content =
        std::fs::read_to_string(&conf).with_context(|| format!("config should be generated to {conf:?}"))?;
    assert!(!conf_content.is_empty(), "config should not be empty");
    Ok(())
}
