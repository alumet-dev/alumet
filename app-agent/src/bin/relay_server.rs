use alumet::static_plugins;
use alumet_agent::{
    agent_util::{self, ConfigLoadOptions},
    init_logger,
    options::{cli, config::CommonOpts, Configurator},
};
use clap::{Parser, Subcommand};

type AgentConfig = CommonOpts;

fn main() {
    let plugins = static_plugins![
        plugin_relay::server::RelayServerPlugin,
        plugin_csv::CsvPlugin,
        plugin_influxdb::InfluxDbPlugin,
    ];

    init_logger();
    const BINARY: &str = env!("CARGO_BIN_NAME");
    const VERSION: &str = env!("CARGO_PKG_VERSION");
    log::info!("Starting ALUMET agent '{BINARY}' v{VERSION}");

    // Parse command-line arguments.
    let mut cli_args = Cli::parse();

    // Extract options
    let command = cli_args.command.take().unwrap_or(Command::Run);
    let load_options = ConfigLoadOptions::new(&mut cli_args.common, &plugins).unwrap();

    // Execute the command.
    match command {
        Command::Run => {
            let (agent_config, plugin_configs) = agent_util::load_config::<AgentConfig>(load_options).unwrap();
            let plugins_info = agent_util::PluginsInfo::new(plugins, plugin_configs);
            let agent_builder = agent_util::new_agent(plugins_info, agent_config, cli_args);
            let agent = agent_util::start(agent_builder);
            agent_util::run_until_stop(agent);
        }
        Command::RegenConfig => {
            agent_util::regen_config::<AgentConfig>(load_options).expect("failed to regenerate the config");
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

    /// The socket address to use when binding, without the port.
    #[arg(long)]
    address: Option<String>,
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
                .plugin_config_mut("relay-server")
                .unwrap()
                .insert(String::from("port"), toml::Value::Integer(port.into()));
        }
        if let Some(address) = self.address.take() {
            agent
                .plugin_config_mut("relay-server")
                .unwrap()
                .insert(String::from("address"), toml::Value::String(address));
        }
    }
}
