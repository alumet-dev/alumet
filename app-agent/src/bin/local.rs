use alumet::static_plugins;
use alumet_agent::{
    agent_util::{self, ConfigLoadOptions, PluginsInfo},
    config::merge_override,
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
    // Specify here all the plugins that will be included in the agent during compilation.
    let plugins = static_plugins![
        plugin_rapl::RaplPlugin,
        plugin_perf::PerfPlugin,
        plugin_procfs::ProcfsPlugin,
        plugin_csv::CsvPlugin,
        plugin_socket_control::SocketControlPlugin,
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

    /// Output to save the measurement to.
    #[arg(long)]
    output: Option<String>,
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
        if self.exec_mode && agent.is_plugin_enabled("procfs") {
            let config = agent
                .plugin_config_mut("procfs")
                .expect("plugin procfs is enabled and should have a config");
            let o = toml::toml! {
                processes.strategy = "event"
            };
            merge_override(config, o);
        }
    }
}

impl Configurator for Cli {
    fn configure_pipeline(&mut self, pipeline: &mut alumet::pipeline::Builder) {
        self.common.configure_pipeline(pipeline);
    }

    fn configure_agent(&mut self, agent: &mut alumet::agent::Builder) {
        self.common.configure_agent(agent);

        if agent.is_plugin_enabled("csv") {
            if let Some(output) = self.output.take() {
                let config = agent
                    .plugin_config_mut("csv")
                    .expect("plugin csv is enabled and should have a config");
                config.insert(String::from("output_path"), toml::Value::String(output));
            }
        }
    }
}
