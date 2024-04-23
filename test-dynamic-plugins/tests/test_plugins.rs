use pretty_assertions::assert_str_eq;
use std::{path::Path, process::Command};

#[test]
fn test_plugin_c() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let plugin_dir = crate_dir.join("../test-dynamic-plugin-c");
    let plugin_lib = plugin_dir.join("target/plugin.so");

    println!("make...");
    let build_result = Command::new("make")
        .current_dir(plugin_dir)
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
        "The application crashed:\n{}",
        String::from_utf8_lossy(&cmd_result.stderr)
    );

    // check the app (and plugin) output
    let output = String::from_utf8(cmd_result.stdout).expect("invalid app output");
    println!("{output}");
    check_app_output(output, plugin_name, plugin_version);
}

fn check_app_output(output: String, plugin_name: &str, plugin_version: &str) {
    let lines = output.lines().collect::<Vec<_>>();
    assert_str_eq!("[app] Starting ALUMET", lines[0]);
    assert_str_eq!(
        &format!("[app] dynamic plugin loaded: {plugin_name} version {plugin_version}"),
        lines[1]
    );
    assert_str_eq!("[app] plugin_config: {\"custom_attribute\": String(\"42\")}", lines[2]);
    assert_str_eq!("[app] Starting the pipeline...", lines[3]);
    assert_str_eq!("[app] pipeline started", lines[4]);

    let plugin_init_regex = regex::Regex::new("plugin = 0x[0-9a-zA-Z]+, custom_attribute = 42").unwrap();
    assert!(plugin_init_regex.is_match(lines[5]), "wrong init: '{}'", lines[5]);

    let plugin_start_regex =
        regex::Regex::new("plugin_start begins with plugin = 0x[0-9a-zA-Z]+, custom_attribute = 42").unwrap();
    assert!(plugin_start_regex.is_match(lines[6]), "wrong start: '{}'", lines[6]);

    assert_str_eq!("plugin_start finished successfully", lines[7]);

    let measurement_output_regex =
        regex::Regex::new("\\[\\d+\\] on cpu_package 0 by local_machine , rapl_pkg_consumption\\(id \\d+\\) = \\d+\\.\\d+").unwrap();
    for i in 8..lines.len() - 1 {
        assert!(
            measurement_output_regex.is_match(lines[i]),
            "wrong measurement: '{}' does not match {:?}",
            lines[i],
            measurement_output_regex
        );
    }

    let last_line = lines.last().unwrap();
    assert_str_eq!("plugin Dropped", *last_line);
}
