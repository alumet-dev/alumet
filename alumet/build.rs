use std::{env, fs, path::Path};

fn main() {
    if !cfg!(feature = "dynamic") {
        // do nothing if the `dynamic` feature is disabled
        return;
    }

    let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let out_dir = Path::new("generated");
    fs::create_dir_all(out_dir).unwrap();
    let out_file_path = out_dir.join("alumet-api.h");
    let sym_file_path = out_dir.join("alumet-symbols.txt");

    // Configure cbindgen for C
    let mut cbindgen_config = cbindgen::Config::default();
    cbindgen_config.language = cbindgen::Language::C;

    // Avoid conflicts between enumeration values
    cbindgen_config.enumeration.prefix_with_name = true;

    // Add a PLUGIN_API macro to only export the symbols of the plugin api
    cbindgen_config.after_includes = Some("#define PLUGIN_API __attribute__((visibility(\"default\")))".to_owned());
    
    // Wrap all the declarations in #ifndef
    cbindgen_config.header = Some("#ifndef __ALUMET_API_H\n#define __ALUMET_API_H".to_owned());
    cbindgen_config.trailer = Some("#endif".to_owned());

    // Generate the bindings
    let bindings = cbindgen::Builder::new()
        .with_crate(crate_dir)
        .with_config(cbindgen_config)
        .generate()
        .expect("Unable to generate C bindings for the plugin API");

    // Write the list of symbols for the linker (useful during the compilation of `app-agent`)
    bindings.generate_symfile(sym_file_path);

    // Write the C bindings.
    bindings.write_to_file(out_file_path);

    println!("C-compatible API generated");
}
