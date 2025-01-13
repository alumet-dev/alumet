use alumet::static_plugins;
use alumet_agent::{
    agent_util::{self, ConfigLoadOptions, PluginsInfo},
    init_logger,
    options::{
        cli::{CommonArgs, ExecArgs},
        config::CommonOpts,
        Configurator,
    },
};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};

fn main() {
    let plugins = static_plugins![
        plugin_rapl::RaplPlugin,
        plugin_perf::PerfPlugin,
        plugin_socket_control::SocketControlPlugin,
        plugin_relay::client::RelayClientPlugin,
        plugin_cgroupv2::K8sPlugin,
        plugin_procfs::ProcfsPlugin,
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
            let plugins_info = PluginsInfo::new(plugins, plugin_configs);
            let agent_builder = agent_util::new_agent(plugins_info, agent_config, cli_args);
            let agent = agent_util::start(agent_builder);
            agent_util::run_until_stop(agent);
        }
        Command::Exec(ExecArgs { program, args }) => {
            let (mut agent_config, plugin_configs) = agent_util::load_config::<AgentConfig>(load_options).unwrap();
            agent_config.exec_mode = true;
            let plugins_info = PluginsInfo::new(plugins, plugin_configs);
            let agent_builder = agent_util::new_agent(plugins_info, agent_config, cli_args);
            let agent = agent_util::start(agent_builder);
            agent_util::exec_process(agent, program, args);
        }
        Command::RegenConfig => {
            agent_util::regen_config::<AgentConfig>(load_options).expect("failed to regenerate the config");
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
struct AgentConfig {
    #[serde(flatten)]
    common: CommonOpts,

    #[serde(skip)] // ignore this field when (de)serializing
    exec_mode: bool,
}

/// Command line arguments.
#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    #[command(flatten)]
    common: CommonArgs,

    /// The name that this client will use to identify itself to the server.
    ///
    /// Defaults to the hostname.
    #[arg(long)]
    client_name: Option<String>,

    /// The address and port of the relay server, for instance `127.0.0.1:50051`.
    #[arg(long)]
    relay_server: Option<String>,
}

#[derive(Subcommand, Clone)]
enum Command {
    /// Run the agent and monitor the system.
    ///
    /// This is the default command.
    Run,

    /// Execute a command and observe its process.
    Exec(ExecArgs),

    /// Regenerate the configuration file and stop.
    ///
    /// If the file exists, it will be overwritten.
    RegenConfig,
}

impl Configurator for AgentConfig {
    fn configure_pipeline(&mut self, pipeline: &mut alumet::pipeline::Builder) {
        self.common.configure_pipeline(pipeline);
        if self.exec_mode {
            pipeline.trigger_constraints_mut().allow_manual_trigger = true;
        }
    }

    fn configure_agent(&mut self, agent: &mut alumet::agent::Builder) {
        self.common.configure_agent(agent);
    }
}

impl Configurator for Cli {
    fn configure_pipeline(&mut self, pipeline: &mut alumet::pipeline::Builder) {
        self.common.configure_pipeline(pipeline);
    }

    fn configure_agent(&mut self, agent: &mut alumet::agent::Builder) {
        self.common.configure_agent(agent);

        // Override some config options with the CLI arguments.
        if let Some(name) = self.client_name.take() {
            agent
                .plugin_config_mut("relay-client")
                .unwrap()
                .insert(String::from("client_name"), toml::Value::String(name));
        }
        if let Some(uri) = self.relay_server.take() {
            agent
                .plugin_config_mut("relay-client")
                .unwrap()
                .insert(String::from("relay_server"), toml::Value::String(uri));
        }
    }
}
