use alumet::{agent::AgentBuilder, static_plugins};
use app_agent::{
    agent_util, init_logger,
    options::{
        cli::{self, ExecArgs},
        config::{self, CommonArgs},
    },
};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};

fn main() {
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
            config.app_config_mut().remove("exec_mode_plugins"); // these overrides only apply to the exec mode
            let agent = agent_util::start(agent, config);
            agent_util::run(agent);
        }
        Command::Exec(ExecArgs { program, args }) => {
            agent.source_trigger_constraints().allow_manual_trigger = true;
            let config = agent_util::load_config::<AppConfig, _>(&mut agent, cli_args.common);
            let agent = agent_util::start(agent, config);
            agent_util::exec(agent, program, args);
        }
        Command::RegenConfig => {
            agent_util::regen_config(agent);
        }
    }
}

#[derive(Serialize, Deserialize)]
struct AppConfig {
    #[serde(flatten)]
    inner: config::CommonArgs,

    /// Overrides the plugin configurations when exec mode is used.
    exec_mode_plugins: Option<toml::Table>,
}

/// Command line arguments.
#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    #[command(flatten)]
    common: cli::CommonArgs,
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

impl super::AgentModifier for AppConfig {
    /// Applies the common config options to the agent.
    fn apply_to(self, agent: &mut Agent, config: &mut AgentConfig) {
        agent.source_trigger_constraints().max_update_interval = self.max_update_interval;

        if let Some(override_table) = self.exec_mode_plugins {
            for (key, value) in override_table {
                let plugin_table = config.plugin_config_mut(&key);
                // TODO if there is no plugin_table we should probably insert the override
                match (plugin_table, value) {
                    (Some(plugin_table), toml::Value::Table(plugin_table_override)) => {
                        config_ops::merge_override(plugin_table, plugin_table_override);
                    }
                    (_, _) => (),
                }
            }
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            inner: CommonArgs::default(),
            // TODO change this depending on the list of plugins (should probably be accessible from the Agent)
            exec_mode_plugins: Some(toml! {
                procfs.processes.strategy = "event"
            }),
        }
    }
}
