use alumet::static_plugins;
use alumet_agent::init_logger;

fn main() {
    init_logger();
    const BINARY: &str = env!("CARGO_BIN_NAME");
    let (version, details) = agent_version();
    log::info!("Starting ALUMET agent '{BINARY}' v{version} ({details})");
}

fn agent_version() -> (String, String) {
    const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");
    const CARGO_DEBUG: &str = env!("VERGEN_CARGO_DEBUG");
    if option_env!("ALUMET_AGENT_RELEASE").is_some() {
        const BUILD_DATE: &str = env!("VERGEN_BUILD_DATE");
        (
            String::from(CRATE_VERSION),
            format!("{BUILD_DATE}, debug={CARGO_DEBUG}"),
        )
    } else {
        let git_hash: &str = option_env!("VERGEN_GIT_SHA").unwrap_or("?");
        const GIT_DIRTY: &str = env!("VERGEN_GIT_DIRTY");
        const BUILD_TIMESTAMP: &str = env!("VERGEN_BUILD_TIMESTAMP");
        const RUSTC_SEMVER: &str = env!("VERGEN_RUSTC_SEMVER");
        let dirty = if GIT_DIRTY == "true" { "-dirty" } else { "" };
        (
            format!("{CRATE_VERSION}-{git_hash}{dirty}"),
            format!("{BUILD_TIMESTAMP}, rustc {RUSTC_SEMVER}, debug={CARGO_DEBUG}"),
        )
    }
}
