use common::run::cargo_run;

mod common;

#[test]
fn help() {
    let status = cargo_run("alumet-relay-server", &["relay_server"], &["--help"]);
    assert!(status.success());
}

#[test]
fn args_bad_config_no_folder() -> anyhow::Result<()> {
    common::tests::args_bad_config_no_folder("alumet-relay-server", &["relay_server"])
}

#[test]
fn args_bad_config_missing_file_no_default() -> anyhow::Result<()> {
    common::tests::args_bad_config_missing_file_no_default("alumet-relay-server", &["relay_server"])
}

#[test]
fn args_regen_config() -> anyhow::Result<()> {
    common::tests::args_regen_config("alumet-relay-server", &["relay_server"])
}
