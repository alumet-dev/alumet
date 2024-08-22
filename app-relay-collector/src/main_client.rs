use std::time::Duration;

use alumet::agent::{static_plugins, Agent, AgentBuilder, AgentConfig};
use alumet::plugin::rust::InvalidConfig;

use clap::Parser;
use env_logger::Env;
use serde::{Deserialize, Serialize};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
    log::info!("Starting ALUMET relay agent v{VERSION}");
    // Print a warning if we are running in debug mode.
    #[cfg(debug_assertions)]
    {
        log::warn!("DEBUG assertions are enabled, this build of Alumet is fine for debugging, but not for production.");
    }

    // Parse command-line arguments.
    let args = Args::parse();

    // Load the relay plugin, and the RAPL one to get some input.
    let plugins = static_plugins![plugin_relay::client::RelayClientPlugin, plugin_rapl::RaplPlugin];

    // Build the measurement agent.
    let mut agent = AgentBuilder::new(plugins)
        .config_path("alumet-agent.toml")
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

    // Start the measurement
    let running_agent = agent.start(agent_config).unwrap_or_else(|err| {
        log::error!("{err:?}");
        if let Some(_) = err.downcast_ref::<InvalidConfig>() {
            log::error!("HINT: You could try to regenerate the configuration by running `{} regen-config` (use --help to get more information).", env!("CARGO_BIN_NAME"));
        }
        panic!("ALUMET relay agent failed to start: {err}");
    });

    // Keep the pipeline running until the app closes.
    running_agent.wait_for_shutdown(Duration::MAX).unwrap();
    log::info!("ALUMET relay agent has stopped.");
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
    if let Some(collector_uri) = cli_args.collector_uri {
        global_config
            .plugin_config_mut("plugin-relay:client")
            .unwrap()
            .insert(String::from("collector_uri"), toml::Value::String(collector_uri));
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

    /// The URI of the collector server, such as `http://127.0.0.1:50051`.
    #[arg(long)]
    collector_uri: Option<String>,
}
