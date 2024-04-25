use alumet::agent::{static_plugins, AgentBuilder};

use env_logger::Env;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
    log::info!("Starting ALUMET relay agent v{VERSION}");

    // Load the relay plugin, and the RAPL one to get some input.
    let plugins = static_plugins![plugin_relay::client::RelayClientPlugin, plugin_rapl::RaplPlugin];

    // Start the agent
    let mut agent = AgentBuilder::new(plugins).config_path("alumet-agent.toml").build();
    let config = agent.load_config().unwrap();
    let mut pipeline = agent.start(config);

    // Keep the pipeline running until the app closes.
    pipeline.wait_for_all();
    log::info!("ALUMET relay agent has stopped.");
}
