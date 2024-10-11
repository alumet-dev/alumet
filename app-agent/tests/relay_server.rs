use common::cargo_run;

mod common;

#[test]
fn help() {
    let status = cargo_run("alumet-relay-server", &["relay_server"], &["--help"]);
    assert!(status.success());
}
