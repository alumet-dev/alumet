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
    let app_file = crate_dir.join("../target/debug/test-dynamic-plugins").canonicalize().unwrap();
    
    // build the cargo args
    let plugin_lib_path = plugin_lib_path.to_str().unwrap();
    let run_args = vec![plugin_lib_path, plugin_name, plugin_version];

    // execute cargo <args>
    println!("command: {:?} {}", app_file, run_args.join(" "));
    println!("");
    let output = Command::new(app_file.to_str().unwrap())
        .args(run_args)
        .current_dir(app_dir)
        .output()
        .expect("Running `cargo run` with the plugin failed");
    assert!(
        output.status.success(),
        "The application crashed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    // check the app (and plugin) output
    let output_str = String::from_utf8(output.stdout).expect("invalid app output");
    println!("{output_str}");
    // assert_str_eq!(
    //     output_str,
    //     expected_app_output(plugin_lib_str, plugin_name, plugin_version)
    // );
}
