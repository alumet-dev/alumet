use alumet::agent::{static_plugins, AgentBuilder};

use env_logger::Env;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
    log::info!("Starting ALUMET relay collector v{VERSION}");

    // Load the collector plugin, and the CSV plugin to have an output.
    let plugins = static_plugins![plugin_relay::server::RelayServerPlugin, plugin_csv::CsvPlugin];

    // Start the collector
    let agent = AgentBuilder::new(plugins).config_path("alumet-collector.toml").build();
    let mut pipeline = agent.start();

    // Keep the pipeline running until the app closes.
    pipeline.wait_for_all();
    log::info!("ALUMET relay collector has stopped.");
}
