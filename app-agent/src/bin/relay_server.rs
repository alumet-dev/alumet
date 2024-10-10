use std::path::PathBuf;

use alumet::static_plugins;
use app_agent::{
    agent_util,
    config_ops::config_mix,
    init_logger,
    options::{cli, config::CommonOpts, Configurator},
};
use clap::{Parser, Subcommand};

type AgentConfig = CommonOpts;

fn main() {
    let plugins = static_plugins![plugin_relay::server::RelayServerPlugin, plugin_csv::CsvPlugin];

    init_logger();
    const BINARY: &str = env!("CARGO_BIN_NAME");
    const VERSION: &str = env!("CARGO_PKG_VERSION");
    log::info!("Starting ALUMET agent '{BINARY}' v{VERSION}");

    // Parse command-line arguments.
    let mut cli_args = Cli::parse();

    // Get the config path.
    let config_path = PathBuf::from(cli_args.common.config.clone());

    // Execute the command.
    let command = cli_args.command.take().unwrap_or(Command::Run);
    let config_override = cli_args.common.config_override_table(&plugins).unwrap();
    match command {
        Command::Run => {
            let (agent_config, plugin_configs) =
                agent_util::load_config::<AgentConfig>(&config_path, &plugins, config_override).unwrap();
            let plugins_info = agent_util::PluginsInfo::new(plugins, plugin_configs);
            let agent_builder = agent_util::new_agent(plugins_info, agent_config, cli_args);
            let agent = agent_util::start(agent_builder);
            agent_util::run_until_stop(agent);
        }
        Command::RegenConfig => {
            let additional = config_mix(AgentConfig::default(), config_override).unwrap();
            agent_util::regen_config(&config_path, &plugins, additional);
        }
    }
}

/// Command line arguments.
#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    #[command(flatten)]
    common: cli::CommonArgs,

    /// The port to use when biding, for example `50051`.
    #[arg(long)]
    port: Option<u16>,
}

#[derive(Subcommand, Clone)]
enum Command {
    /// Run the agent and monitor the system.
    ///
    /// This is the default command.
    Run,

    /// Regenerate the configuration file and stop.
    ///
    /// If the file exists, it will be overwritten.
    RegenConfig,
}

impl Configurator for Cli {
    fn configure_pipeline(&mut self, pipeline: &mut alumet::pipeline::Builder) {
        self.common.configure_pipeline(pipeline);
    }

    fn configure_agent(&mut self, agent: &mut alumet::agent::Builder) {
        self.common.configure_agent(agent);
        // Override some config options with the CLI args
        if let Some(port) = self.port.take() {
            agent
                .plugin_config_mut("plugin-relay:server")
                .unwrap()
                .insert(String::from("port"), toml::Value::Integer(port.into()));
        }
    }
}
