use std::time::Duration;

use alumet::agent::{static_plugins, Agent, AgentBuilder, AgentConfig};

use env_logger::Env;

use plugin_csv::CsvPlugin;
use plugin_rapl::RaplPlugin;
use plugin_socket_control::SocketControlPlugin;
use serde::{Deserialize, Serialize};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
    log::info!("Starting ALUMET agent v{VERSION}");

    // Specifies the plugins that we want to load.
    let plugins = static_plugins![RaplPlugin, CsvPlugin, SocketControlPlugin];

    // Build the measurement agent.
    let mut agent = AgentBuilder::new(plugins)
        .config_path("alumet-config.toml")
        .default_app_config(AppOptions::default())
        .build();

    // Load the config.
    let mut agent_config = agent.load_config().unwrap();
    apply_config(&mut agent, &mut agent_config);

    // Start the measurement.
    let mut pipeline = agent.start(agent_config);
    log::info!("ALUMET agent is ready.");

    // Keep the pipeline running until the app closes.
    pipeline.wait_for_all();
    log::info!("ALUMET agent has stopped.");
}

fn apply_config(agent: &mut Agent, agent_config: &mut AgentConfig) {
    let options: AppOptions = agent_config.take_app_config().try_into().unwrap();
    agent.sources_max_update_interval(options.max_update_interval);
}

#[derive(Deserialize, Serialize)]
struct AppOptions {
    #[serde(with = "humantime_serde")]
    max_update_interval: Duration,
}

impl Default for AppOptions {
    fn default() -> Self {
        Self {
            max_update_interval: Duration::from_millis(500),
        }
    }
}
