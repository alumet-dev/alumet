use anyhow::Context;

use crate::common::run::cargo_run_tee;

pub fn args_bad_config_no_folder(binary: &str, features: &[&str]) -> anyhow::Result<()> {
    let tmp_dir = std::env::temp_dir().join(format!("{}-args_bad_config_no_folder", env!("CARGO_CRATE_NAME")));
    let bad_conf = tmp_dir.join("zzzzz.toml");
    let _ = std::fs::remove_dir_all(&tmp_dir);

    let bad_conf_filename = bad_conf.to_str().unwrap();
    let output = cargo_run_tee(binary, features, &["--config", bad_conf_filename])?;
    assert!(
        !output.status.success(),
        "should fail because the config directory does not exist"
    );
    let stdout = String::from_utf8(output.stdout)?;
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stdout.contains(bad_conf_filename) || stderr.contains(bad_conf_filename));
    Ok(())
}

pub fn args_bad_config_missing_file_no_default(binary: &str, features: &[&str]) -> anyhow::Result<()> {
    let tmp_dir = std::env::temp_dir().join(format!(
        "{}-args_bad_config_missing_file_no_default",
        env!("CARGO_CRATE_NAME")
    ));
    let bad_conf = tmp_dir.join("zzzzz.toml");
    let _ = std::fs::remove_file(&bad_conf);

    let bad_conf_filename = bad_conf.to_str().unwrap();
    let output = cargo_run_tee(
        binary,
        features,
        &["--config", bad_conf_filename, "--no-default-config"],
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

pub fn args_regen_config(binary: &str, features: &[&str]) -> anyhow::Result<()> {
    let tmp_dir = std::env::temp_dir().join(format!("{}-args_regen_config", env!("CARGO_CRATE_NAME")));
    let conf = tmp_dir.join("config.toml");
    let _ = std::fs::remove_file(&conf);
    let _ = std::fs::create_dir(&tmp_dir);

    let conf_path_str = conf.to_str().unwrap();
    let output = cargo_run_tee(binary, features, &["--config", conf_path_str, "regen-config"])?;
    assert!(output.status.success(), "command should succeed");

    let conf_content =
        std::fs::read_to_string(&conf).with_context(|| format!("config should be generated to {conf:?}"))?;
    assert!(!conf_content.is_empty(), "config should not be empty");
    Ok(())
}
