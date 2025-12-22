use std::{env, path::PathBuf};

fn main() {
    if env::var("DOCS_RS").is_ok() {
        return;
    }

    let bindings = bindgen::Builder::default()
        .headers(["./include/amdsmi.h", "./include/amd_smiConfig.h"])
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .dynamic_library_name("libamd_smi")
        .raw_line("#![allow(warnings)]")
        .generate()
        .unwrap();

    let out_path = PathBuf::from("./src");
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
