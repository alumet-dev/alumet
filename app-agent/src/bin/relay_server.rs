use alumet::{agent::AgentBuilder, static_plugins};
use app_agent::{
    agent_util, init_logger,
    options::{cli, config, AgentModifier},
};
use clap::{Parser, Subcommand};

type AppConfig = config::CommonArgs;

fn main() {
    let plugins = static_plugins![
        plugin_relay::server::RelayServerPlugin,
        plugin_csv::CsvPlugin,
    ];

    init_logger();
    const BINARY: &str = env!("CARGO_BIN_NAME");
    const VERSION: &str = env!("CARGO_PKG_VERSION");
    log::info!("Starting ALUMET agent '{BINARY}' v{VERSION}");

    // Parse command-line arguments.
    let cli_args = Cli::parse();

    // Prepare the plugins and the config.
    let mut agent = AgentBuilder::new(plugins)
        .config_path(&cli_args.common.config)
        .default_app_config(AppConfig::default())
        .build();

    // Execute the command.
    let command = cli_args.command.unwrap_or(Command::Run);
    match command {
        Command::Run => {
            let config = agent_util::load_config::<AppConfig, _>(&mut agent, cli_args.common);
            let agent = agent_util::start(agent, config);
            agent_util::run(agent);
        }
        Command::RegenConfig => {
            agent_util::regen_config(agent);
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

impl AgentModifier for Cli {
    fn apply_to(self, agent: &mut alumet::agent::Agent, config: &mut alumet::agent::AgentConfig) {
        self.common.apply_to(agent, config);

        // Override the config with CLI args, if any.
        if let Some(port) = self.port {
            config
                .plugin_config_mut("plugin-relay:server")
                .unwrap()
                .insert(String::from("port"), toml::Value::Integer(port.into()));
        }
    }
}
