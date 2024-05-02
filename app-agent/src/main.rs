use std::time::Duration;

use alumet::agent::{static_plugins, Agent, AgentBuilder, AgentConfig};

use clap::Parser;
use env_logger::Env;

use plugin_csv::CsvPlugin;
use plugin_rapl::RaplPlugin;
use plugin_socket_control::SocketControlPlugin;
use serde::{Deserialize, Serialize};
use plugin_k8s::K8sPlugin;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
    log::info!("Starting ALUMET agent v{VERSION}");

    // Parse command-line arguments.
    let args = Args::parse();

    // Specifies the plugins that we want to load.
    log::info!("Starting plugins...");
    let plugins = static_plugins![RaplPlugin, CsvPlugin, SocketControlPlugin];

    // Build the measurement agent.
    let mut agent = AgentBuilder::new(plugins)
        .config_path("alumet-config.toml")
        .default_app_config(AppConfig::default())
        .build();

    // CLI option: config regeneration.
    if args.regen_config {
        agent
            .write_default_config()
            .expect("failed to (re)generate the configuration file");
        log::info!("Configuration file (re)generated.");
        return;
    }

    // Load the config.
    let mut agent_config = agent.load_config().unwrap();
    apply_config(&mut agent, &mut agent_config, args);

    // Start the measurement.
    let running_agent = agent.start(agent_config);
    log::info!("ALUMET agent is ready.");

    // Keep the pipeline running until the app closes.
    running_agent.wait_for_shutdown().unwrap();
    log::info!("ALUMET agent has stopped.");
}

/// Applies the configuration (file + arguments).
fn apply_config(agent: &mut Agent, global_config: &mut AgentConfig, cli_args: Args) {
    // Apply the config file
    let app_config: AppConfig = global_config.take_app_config().try_into().unwrap();
    agent.sources_max_update_interval(app_config.max_update_interval);

    // Apply the CLI args (they override the file)
    if let Some(max_update_interval) = cli_args.max_update_interval {
        agent.sources_max_update_interval(max_update_interval);
    }
}

/// Structure of the config file, excluding plugin configs.
#[derive(Deserialize, Serialize)]
struct AppConfig {
    #[serde(with = "humantime_serde")]
    max_update_interval: Duration,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            max_update_interval: Duration::from_millis(500),
        }
    }
}

/// Command line arguments.
#[derive(Parser)]
struct Args {
    /// Regenerate the configuration file and stop.
    ///
    /// If the file exists, it will be overwritten.
    #[arg(long)]
    regen_config: bool,

    /// Maximum amount of time between two updates of the sources' commands.
    ///
    /// A lower value means that the latency of source commands will be lower,
    /// i.e. commands will be applied faster, at the cost of a higher overhead.
    #[arg(long, value_parser = humantime_serde::re::humantime::parse_duration)]
    max_update_interval: Option<Duration>,
}
