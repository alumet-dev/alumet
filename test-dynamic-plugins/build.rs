use std::path::Path;

fn main() {
    // Build alumet_ffi with a custom environment variable first
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo_dir = crate_dir.parent().unwrap();
    let bindgen_out_dir = repo_dir.join("target/tmp/alumet_ffi_build/ffi_generated");
    let alumet_ffi_header = bindgen_out_dir.join("alumet-api.h");
    let alumet_ffi_symbols = bindgen_out_dir.join("alumet-symbols.txt");

    let header_path = alumet_ffi_header.canonicalize().unwrap();
    let symfile_path = alumet_ffi_symbols.canonicalize().unwrap();

    // Tell cargo to invalidate the build when the header changes
    println!("cargo:rerun-if-changed={}", header_path.to_str().unwrap());

    // Add link flags
    let linker_flags = format!("-Wl,--dynamic-list={}", symfile_path.to_str().unwrap());
    println!("cargo:rustc-link-arg={}", linker_flags);
}
