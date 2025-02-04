use std::error::Error;

use vergen::{BuildBuilder, CargoBuilder, Emitter, RustcBuilder};
use vergen_gitcl::GitclBuilder;

fn main() {
    emit_build_info().expect("failed to emit build information");
}

/// Emit cargo instructions that allow the crate to access
/// build-related information at compile-time.
fn emit_build_info() -> Result<(), Box<dyn Error>> {
    let build = BuildBuilder::default().build_date(true).build_timestamp(true).build()?;
    let cargo = CargoBuilder::default().debug(true).build()?;
    let git = GitclBuilder::default().sha(true).dirty(true).build()?;
    let rustc = RustcBuilder::default().semver(true).build()?;

    Emitter::default()
        .add_instructions(&build)?
        .add_instructions(&cargo)?
        .add_instructions(&git)?
        .add_instructions(&rustc)?
        .emit()?;

    Ok(())
}
