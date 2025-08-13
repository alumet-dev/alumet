use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    println!("cargo::rerun-if-env-changed=SKIP_BINDGEN");
    println!("cargo::rerun-if-env-changed=BINDGEN_OUT_DIR");
    println!("cargo::rerun-if-env-changed=ADDITIONAL_TARGET_DIR");

    if env::var_os("SKIP_BINDGEN").is_some() {
        return;
    }

    let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let target_dir = env::var("BINDGEN_OUT_DIR").unwrap_or_else(|_| env::var("OUT_DIR").unwrap());
    let additional_out_dir = match env::var("ADDITIONAL_TARGET_DIR") {
        Err(_) => None,
        Ok(_) => Some(
            PathBuf::from(crate_dir.clone())
                .parent()
                .unwrap()
                .join("target/tmp/alumet_ffi_build/ffi_generated"),
        ),
    };
    let out_dir = Path::new(&target_dir).join("ffi_generated");
    let out_file_path = out_dir.join("alumet.h");
    let sym_file_path = out_dir.join("alumet-symbols.txt");
    fs::create_dir_all(&out_dir).unwrap();

    // Export metadata for dependents crates: give them the path to the header and symbols.
    println!("cargo::metadata=BINDINGS_DIR={}", out_dir.display());

    // Configure cbindgen for C
    let mut cbindgen_config = cbindgen::Config {
        language: cbindgen::Language::C,
        ..Default::default()
    };

    // Avoid conflicts between enumeration values
    cbindgen_config.enumeration.prefix_with_name = true;

    // Add a PLUGIN_API macro to only export the symbols of the plugin api
    cbindgen_config.after_includes = Some("#define PLUGIN_API __attribute__((visibility(\"default\")))".to_owned());

    // Wrap all the declarations in #ifndef
    cbindgen_config.header = Some("#ifndef __ALUMET_API_H\n#define __ALUMET_API_H".to_owned());
    cbindgen_config.trailer = Some("#endif".to_owned());
    cbindgen_config.parse.expand.crates.push(String::from("alumet_ffi"));

    // Generate the bindings
    let bindings = with_rustc_bootstrap(|| {
        cbindgen::Builder::new()
            .with_crate(crate_dir)
            .with_config(cbindgen_config)
            .generate()
            .expect("Unable to generate C bindings for the plugin API")
    });

    // Write the list of symbols for the linker (useful during the compilation of the agent)
    bindings.generate_symfile(sym_file_path);

    // Write the C bindings.
    bindings.write_to_file(out_file_path);
    println!("C-compatible API generated");

    // Copy to additional dir
    if let Some(out_dir) = additional_out_dir {
        let out_file_path = out_dir.join("alumet.h");
        let sym_file_path = out_dir.join("alumet-symbols.txt");
        bindings.generate_symfile(sym_file_path);
        bindings.write_to_file(out_file_path);
    }
}

/// Enable flag RUSTC_BOOTSTRAP, which allows to use nightly API on the stable compiler.
/// This is necessary to expand macros, see:
/// - https://github.com/dtolnay/cargo-expand/pull/183/files
/// - https://github.com/rust-lang/rust/issues/43364
fn with_rustc_bootstrap<R>(f: impl FnOnce() -> R) -> R {
    let previous_bootstrap = env::var("RUSTC_BOOTSTRAP").ok();
    unsafe {
        env::set_var("RUSTC_BOOTSTRAP", "1");
    }

    let res = f();

    if let Some(prev) = previous_bootstrap {
        unsafe {
            env::set_var("RUSTC_BOOTSTRAP", prev);
        }
    } else {
        unsafe {
            env::remove_var("RUSTC_BOOTSTRAP");
        }
    }

    res
}
