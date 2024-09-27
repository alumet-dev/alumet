use alumet::{agent::AgentBuilder, static_plugins};
use app_agent::{
    agent_util, init_logger,
    options::{
        cli::{self, ExecArgs},
        config, AgentModifier,
    },
};
use clap::{Parser, Subcommand};

type AppConfig = config::CommonArgs;

fn main() {
    let plugins = static_plugins![
        plugin_rapl::RaplPlugin,
        plugin_perf::PerfPlugin,
        plugin_socket_control::SocketControlPlugin,
        plugin_relay::client::RelayClientPlugin,
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
    let command = cli_args.command.clone().unwrap_or(Command::Run);
    match command {
        Command::Run => {
            let config = agent_util::load_config::<AppConfig, _>(&mut agent, cli_args);
            let agent = agent_util::start(agent, config);
            agent_util::run(agent);
        }
        Command::Exec(ExecArgs { program, args }) => {
            agent.source_trigger_constraints().allow_manual_trigger = true;
            let config = agent_util::load_config::<AppConfig, _>(&mut agent, cli_args);
            let agent = agent_util::start(agent, config);
            agent_util::exec(agent, program, args);
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

    /// The name that this client will use to identify itself to the collector server.
    ///
    /// Defaults to the hostname.
    #[arg(long)]
    client_name: Option<String>,

    /// The URI of the collector, for instance `http://127.0.0.1:50051`.
    #[arg(long)]
    collector_uri: Option<String>,
}

#[derive(Subcommand, Clone)]
enum Command {
    /// Run the agent and monitor the system.
    ///
    /// This is the default command.
    Run,

    /// Execute a command and observe its process.
    Exec(cli::ExecArgs),

    /// Regenerate the configuration file and stop.
    ///
    /// If the file exists, it will be overwritten.
    RegenConfig,
}

impl AgentModifier for Cli {
    fn apply_to(self, agent: &mut alumet::agent::Agent, config: &mut alumet::agent::AgentConfig) {
        self.common.apply_to(agent, config);

        // Override the config with CLI args, if any.
        if let Some(name) = self.client_name {
            config
                .plugin_config_mut("plugin-relay:client")
                .unwrap()
                .insert(String::from("client_name"), toml::Value::String(name));
        }
        if let Some(uri) = self.collector_uri {
            config
                .plugin_config_mut("plugin-relay:client")
                .unwrap()
                .insert(String::from("collector_uri"), toml::Value::String(uri));
        }
    }
}
