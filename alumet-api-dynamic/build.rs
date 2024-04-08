use std::{path::{PathBuf, Path}, env};

fn main() {
    // the current directory is the one containing build.rs
    let libdir_path = PathBuf::from("../alumet").canonicalize().unwrap();
    let header_path = libdir_path.join("generated/alumet-api.h");
    let out_dir = env::var("OUT_DIR").unwrap();
    let outfile_path = Path::new(&out_dir).join("bindings.rs");

    let libdir_path_str = libdir_path.to_str().unwrap();
    let header_path_str = header_path.to_str().unwrap();

    // Tell cargo to look for shared libraries in the specified directory
    println!("cargo:rustc-link-search={}", libdir_path_str);

    // Tell cargo to invalidate the built crate whenever the header changes.
    println!("cargo:rerun-if-changed={}", header_path_str);

    // Generate the Rust bindings with bindgen.
    let bindings = bindgen::Builder::default()
        .header(header_path_str)
        .allowlist_file(header_path_str)
        .layout_tests(false)
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("Unable to generate Rust bindings for C header");

    bindings.write_to_file(&outfile_path).expect("Failed to write the bindings");
    
    // Create a link to the bindings for easy inspection
    let link_path = PathBuf::from("./generated/bindings.rs");
    std::fs::create_dir_all(link_path.parent().unwrap()).unwrap();
    let _ = std::fs::remove_file(&link_path);
    #[allow(deprecated)]
    std::fs::soft_link(&outfile_path, &link_path).expect("Failed to create link to bindings.rs");
}
