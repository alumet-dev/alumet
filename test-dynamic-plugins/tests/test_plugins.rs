use pretty_assertions::assert_str_eq;
use std::path::{Path, PathBuf};
use std::{io, process::Command};

#[test]
fn test_plugin_c() {
    fn is_dir_empty(p: &Path) -> io::Result<bool> {
        Ok(std::fs::read_dir(p)?.count() == 0)
    }

    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let plugin_dir = crate_dir.join("../test-dynamic-plugin-c");
    let plugin_lib = plugin_dir.join("target/plugin.so");
    let bindgen_out_dir = PathBuf::from(env!("ALUMET_H_BINDINGS_DIR"));

    assert!(
        !is_dir_empty(&bindgen_out_dir).unwrap(),
        "{bindgen_out_dir:?} should not be empty"
    );

    println!("make...");
    let build_result = Command::new("make")
        .current_dir(plugin_dir)
        .env("ALUMET_H_BINDINGS_DIR", bindgen_out_dir)
        .spawn()
        .expect("Running `make` failed")
        .wait()
        .unwrap();
    assert!(build_result.success(), "Building C plugin failed");

    println!("cargo build...");
    let build_result = Command::new("cargo")
        .current_dir(crate_dir)
        .arg("build")
        .spawn()
        .expect("Running `make` failed")
        .wait()
        .unwrap();
    assert!(build_result.success(), "Building C plugin failed");

    run_app_with_plugin(&plugin_lib, "test-dynamic-plugin-c", "0.1.0");
}

fn run_app_with_plugin(plugin_lib: &Path, plugin_name: &str, plugin_version: &str) {
    // check paths
    let plugin_lib_path = plugin_lib
        .canonicalize()
        .expect(&format!("plugin not found: {}", plugin_lib.to_string_lossy()));
    println!("plugin: {plugin_lib_path:?}");

    // find the right directory
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let app_dir = crate_dir;
    let app_file = crate_dir
        .join("../target/debug/test-dynamic-plugins")
        .canonicalize()
        .unwrap();

    // build the cargo args
    let plugin_lib_path = plugin_lib_path.to_str().unwrap();
    let run_args = vec![plugin_lib_path, plugin_name, plugin_version];

    // execute cargo <args>
    println!("command: {:?} {}", app_file, run_args.join(" "));
    println!("");
    let cmd_result = Command::new(app_file.to_str().unwrap())
        .args(run_args)
        .current_dir(app_dir)
        .output()
        .expect("Running `cargo run` with the plugin failed");
    assert!(
        cmd_result.status.success(),
        "The application crashed:\n---[stderr]---\n{}\n---[stdout]---\n{}",
        String::from_utf8_lossy(&cmd_result.stderr),
        String::from_utf8_lossy(&cmd_result.stdout),
    );

    // check the app (and plugin) output
    let output = String::from_utf8(cmd_result.stdout).expect("invalid app output");
    println!("\n---[stdout]---\n{output}");
    let out_err = String::from_utf8(cmd_result.stderr).expect("invalid app stderr");
    println!("\n---[stderr]---\n{out_err}");
    check_app_output(output, plugin_name, plugin_version);
}

fn check_app_output(output: String, plugin_name: &str, plugin_version: &str) {
    let lines = output.lines().collect::<Vec<_>>();
    assert_str_eq!("[app] Starting ALUMET", lines[0]);
    assert_str_eq!(
        &format!("[app] dynamic plugin loaded: {plugin_name} version {plugin_version}"),
        lines[1]
    );
    assert_str_eq!("[app] plugin config: {\"custom_attribute\": String(\"42\")}", lines[2]);

    let plugin_init_regex = regex::Regex::new("plugin = 0x[0-9a-zA-Z]+, custom_attribute = 42").unwrap();
    assert!(plugin_init_regex.is_match(lines[3]), "wrong init: '{}'", lines[3]);

    let plugin_start_regex =
        regex::Regex::new("plugin_start begins with plugin = 0x[0-9a-zA-Z]+, custom_attribute = 42").unwrap();
    assert!(plugin_start_regex.is_match(lines[4]), "wrong start: '{}'", lines[4]);

    assert_str_eq!("plugin_start finished successfully", lines[5]);

    assert_str_eq!("[app] plugin started", lines[6]);
    assert_str_eq!("[app] Starting the pipeline...", lines[7]);
    assert_str_eq!("[app] pipeline started", lines[8]);

    let measurement_output_regex = regex::Regex::new(
        "\\[\\d+\\] on cpu_package 0 by local_machine , rapl_pkg_consumption\\(id \\d+\\) = \\d+\\.\\d+",
    )
    .unwrap();
    for i in 9..lines.len() - 3 {
        let line = lines[i];
        let is_measurement = measurement_output_regex.is_match(line);
        assert!(
            is_measurement || line == "[app] shutting down...",
            "wrong measurement: '{}' does not match {:?}",
            line,
            measurement_output_regex
        );
    }

    let line_pstop = lines[lines.len() - 3];
    let line_pdrop = lines[lines.len() - 2];
    let line_last = lines[lines.len() - 1];
    assert_str_eq!("plugin stopped", line_pstop);
    assert_str_eq!("plugin Dropped", line_pdrop);
    assert_str_eq!("[app] stop", line_last);
}
