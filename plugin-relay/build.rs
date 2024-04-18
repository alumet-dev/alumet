use std::path::PathBuf;

/// When building the Rust project, compile the protobuf files.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto_path = &PathBuf::from("proto/alumet-relay.proto");

    // directory the main .proto file resides in
    let proto_dir = proto_path.parent().expect("proto file should reside in a directory");

    tonic_build::configure()
        .protoc_arg("--experimental_allow_proto3_optional")
        .compile(&[proto_path], &[proto_dir])?;
    Ok(())
}
