use std::path::PathBuf;

fn main() {
    let header_path = PathBuf::from("../alumet/generated/alumet-api.h").canonicalize().unwrap();
    let symfile_path = PathBuf::from("../alumet/generated/alumet-symbols.txt").canonicalize().unwrap();

    // Tell cargo to invalidate the build when the header changes
    println!("cargo:rerun-if-changed={}", header_path.to_str().unwrap());

    // Add link flags
    let linker_flags = format!("-Wl,--dynamic-list={}", symfile_path.to_str().unwrap());
    println!("cargo:rustc-link-arg={}", linker_flags);
}
