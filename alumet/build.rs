use std::{fs, env, path::Path};

use cbindgen::Language::C;

fn main() {
    let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let out_dir = Path::new("generated");
    fs::create_dir_all(out_dir).unwrap();
    let out_file_path = out_dir.join("alumet-api.h");
    let sym_file_path = out_dir.join("alumet-symbols.txt");

    let bindings = cbindgen::Builder::new()
        .with_crate(crate_dir)
        .with_language(C)
        .generate()
        .expect("Unable to generate C bindings for the plugin API");

    // Write the list of symbols for the linker (useful during the compilation of `app`)
    bindings.generate_symfile(sym_file_path);
    
    // Write the C bindings.
    bindings.write_to_file(out_file_path);
    
    println!("C-compatible API generated");
}
