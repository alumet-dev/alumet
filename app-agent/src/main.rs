use alumet::agent::{static_plugins, AgentBuilder};

use env_logger::Env;

use plugin_csv::CsvPlugin;
use plugin_rapl::RaplPlugin;
use plugin_socket_control::SocketControlPlugin;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
    log::info!("Starting ALUMET agent v{VERSION}");

    // Specifies the plugins that we want to load.
    let plugins = static_plugins![RaplPlugin, CsvPlugin, SocketControlPlugin];

    // Read the config file.
    let config_path = std::path::Path::new("alumet-config.toml");
    let file_content = std::fs::read_to_string(config_path).unwrap_or("".to_owned()); //.expect("failed to read file");
    let config: toml::Table = file_content.parse().unwrap();

    // Start the measurement agent.
    let agent = AgentBuilder::new(plugins, config).build();
    let mut pipeline = agent.start();
    log::info!("ALUMET agent is ready.");

    // Keep the pipeline running until the app closes.
    pipeline.wait_for_all();
    log::info!("ALUMET agent has stopped.");
}
