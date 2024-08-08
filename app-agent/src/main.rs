use std::{process, time::Duration};

use alumet::{
    agent::{static_plugins, Agent, AgentBuilder, AgentConfig},
    plugin::{
        event::{self, StartConsumerMeasurement},
        rust::InvalidConfig,
    },
    resources::ResourceConsumer,
};

use clap::{Args, Parser, Subcommand};
use env_logger::Env;

use plugin_csv::CsvPlugin;
use plugin_perf::PerfPlugin;
use plugin_rapl::RaplPlugin;
use plugin_socket_control::SocketControlPlugin;
use serde::{Deserialize, Serialize};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
    log::info!("Starting ALUMET agent v{VERSION}");

    // Parse command-line arguments.
    let args = Cli::parse();

    // Specifies the plugins that we want to load.
    let plugins = static_plugins![RaplPlugin, CsvPlugin, PerfPlugin, SocketControlPlugin];

    // Build the measurement agent.
    let mut agent = AgentBuilder::new(plugins)
        .config_path(&args.config)
        .default_app_config(AppConfig::default())
        .build();

    // CLI option
    let cmd = args.command.clone().unwrap_or(Commands::Run);

    if matches!(cmd, Commands::RegenConfig) {
        // Regenerate config and stop.
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
    let running_agent = agent.start(agent_config).unwrap_or_else(|err| {
        log::error!("{err:?}");
        if let Some(_) = err.downcast_ref::<InvalidConfig>() {
            log::error!("HINT: You could try to regenerate the configuration by running `{} regen-config` (use --help to get more information).", env!("CARGO_BIN_NAME"));
        }
        panic!("ALUMET agent failed to start: {err}");
    });

    // Keep the pipeline running until...
    match cmd {
        Commands::Run => {
            // ...the program stops (on SIGTERM or on a "stop" command).
            running_agent.wait_for_shutdown().unwrap();
        }
        Commands::Exec(ExecArgs {
            program: external_command,
            args,
        }) => {
            // ...another process, that we'll launch now, exits.

            // Spawn the process.
            let mut p = process::Command::new(external_command.clone())
                .args(args)
                .spawn()
                .expect("error in child process");

            // Notify the plugins that there is a process to observe.
            let pid = p.id();
            log::info!("Child process '{external_command}' spawned with pid {pid}.");
            event::start_consumer_measurement()
                .publish(StartConsumerMeasurement(vec![ResourceConsumer::Process { pid }]));

            // Wait for the process to terminate.
            let status = p.wait().expect("failed to wait for child process");
            log::info!("Child process exited with status {status}, Alumet will now stop.");

            // Stop the pipeline.
            running_agent.pipeline.control_handle().shutdown();
            running_agent.wait_for_shutdown().unwrap();
        }
        Commands::RegenConfig => unreachable!(),
    }
    log::info!("ALUMET agent has stopped.")
}

/// Applies the configuration (file + arguments).
fn apply_config(agent: &mut Agent, global_config: &mut AgentConfig, cli_args: Cli) {
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
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Path to the config file.
    #[arg(long, default_value = "alumet-config.toml")]
    config: String,
    /// Maximum amount of time between two updates of the sources' commands.
    ///
    /// A lower value means that the latency of source commands will be lower,
    /// i.e. commands will be applied faster, at the cost of a higher overhead.
    #[arg(long, value_parser = humantime_serde::re::humantime::parse_duration)]
    max_update_interval: Option<Duration>,
}

#[derive(Subcommand, Clone)]
enum Commands {
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

#[derive(Args, Clone)]
struct ExecArgs {
    /// The program to run.
    program: String,

    /// Arguments to the program.
    #[arg(trailing_var_arg = true)]
    args: Vec<String>,
}
