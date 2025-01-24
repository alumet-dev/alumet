use std::{str::FromStr, time::Duration};

use alumet::{
    agent::{
        config::{merge_override, AutoDefaultConfigProvider, DefaultConfigProvider},
        exec,
        plugin::{PluginSet, PluginStatus, UnknownPluginInConfigPolicy},
    },
    pipeline, static_plugins,
};
use alumet_agent::{exec_hints, init_logger};
use anyhow::Context;
use clap::{Args, FromArgMatches};
use cli::{ConfigArgs, ConfigCommand, PluginsArgs, PluginsCommand};
use config::GeneralConfig;

const BINARY: &str = env!("CARGO_BIN_NAME");

fn main() {
    init_logger();

    let plugins = static_plugins![
        #[cfg(feature = "socket-control")]
        plugin_socket_control::SocketControlPlugin,
        #[cfg(feature = "k8s")]
        plugin_cgroupv2::K8sPlugin,
        #[cfg(feature = "oar3")]
        plugin_cgroupv2::OARPlugin,
        #[cfg(feature = "oar2")]
        plugin_oar2::Oar2Plugin,
        #[cfg(feature = "rapl")]
        plugin_rapl::RaplPlugin,
        #[cfg(feature = "linux-inputs")]
        plugin_perf::PerfPlugin,
        #[cfg(feature = "linux-inputs")]
        plugin_procfs::ProcfsPlugin,
        #[cfg(any(feature = "nvidia-nvml", feature = "nvidia-jetson"))]
        plugin_nvidia::NvidiaPlugin,
        #[cfg(feature = "output-csv")]
        plugin_csv::CsvPlugin,
        #[cfg(feature = "output-influxdb")]
        plugin_influxdb::InfluxDbPlugin,
        #[cfg(feature = "output-mongodb")]
        plugin_mongodb::MongoDbPlugin,
        #[cfg(feature = "relay-client")]
        plugin_relay::client::RelayClientPlugin,
        #[cfg(feature = "relay-server")]
        plugin_relay::server::RelayServerPlugin,
    ];
    let mut plugins = PluginSet::new(plugins);

    // Define the command-line interface.
    let mut cmd = clap::Command::new(BINARY).version(agent_version());
    cmd = cli::Cli::augment_args(cmd);

    // Parse CLI arguments and handle some special flags like --version and --help.
    let matches = cmd.get_matches();
    let mut args = cli::Cli::from_arg_matches(&matches).map_err(|e| e.exit()).unwrap();

    // Special flags like --help will exit. In other cases, we continue.
    print_welcome();

    // Run CLI commands that run before the config is loaded.
    if run_command_no_config(&args, &plugins).expect("command failed") {
        return;
    }

    // apply some settings that may change how the config file is parsed
    // or how the default config file is generated
    let config_override = parse_config_overrides(&args).expect("invalid config overrides");
    if let Some(enabled_plugins) = &args.common.plugins {
        plugins.enable_only(enabled_plugins);
    }

    // parse config file
    let default_config_provider = AutoDefaultConfigProvider::<config::GeneralConfig>::new(&plugins);
    let mut config = alumet::agent::config::Loader::parse_file(&args.common.config)
        .or_default(default_config_provider, true)
        .substitute_env_variables(true)
        .with_override(config_override)
        .load()
        .expect("could not load config file");

    // Extract the config of each plugin.
    // If not set by CLI args, use the config to determine which plugins are enabled.
    plugins
        .extract_config(
            &mut config,
            args.common.plugins.is_none(),
            UnknownPluginInConfigPolicy::Error,
        )
        .expect("invalid plugins config");

    // Extract non-plugin config.
    let config = config.try_into::<GeneralConfig>().expect("invalid general config");

    // Run CLI commands that only require the config and run before the pipeline starts.
    if run_command_no_measurement(&args, &config, &plugins).expect("command failed") {
        return;
    }

    // begin the creation of the pipeline (we have some settings to apply to it)
    let mut pipeline = alumet::pipeline::Builder::new();
    apply_pipeline_settings(&args, &config, &mut pipeline);

    // start Alumet with the pipeline and plugins
    let agent = alumet::agent::Builder::from_pipeline(plugins, pipeline)
        .build_and_start()
        .expect("startup failure");

    // run the provided command, the default is Run
    match args.command.take().unwrap_or(cli::Command::Run) {
        cli::Command::Run => {
            // execute the pipeline until Alumet is externally stopped (e.g. by Ctrl+C)
            agent.wait_for_shutdown(Duration::MAX).expect("error while running");
        }
        cli::Command::Exec(exec_args) => {
            let timeout = Duration::from_secs(5);
            let res = exec::watch_process(agent, exec_args.program, exec_args.args, timeout);
            if let Err(err @ exec::WatchError::ProcessSpawn(program, e)) = &res {
                match e.kind() {
                    std::io::ErrorKind::NotFound => {
                        panic!("{}", exec_hints::handle_not_found(program.clone(), Vec::new()));
                    }
                    std::io::ErrorKind::PermissionDenied => {
                        panic!("{}", exec_hints::handle_permission_denied(program.clone()));
                    }
                    _ => {
                        panic!("{}", err);
                    }
                }
            }
        }
        _ => unreachable!("every command should have been handled at this point"),
    }
}

/// Prints a short welcome message.
fn print_welcome() {
    // It is useful to have the precise version of the agent in the logs.
    log::info!("Starting Alumet agent '{BINARY}' v{}", agent_version());

    // Print a warning if we are running in debug mode.
    #[cfg(debug_assertions)]
    {
        log::warn!("DEBUG assertions are enabled, this build of Alumet is fine for debugging, but not for production.");
    }
}

/// If selected by the CLI user, runs a command that does not need the config file.
///
/// Returns `true` if a command was run (in which case you probably should stop here).
fn run_command_no_config(args: &cli::Cli, plugins: &PluginSet) -> anyhow::Result<bool> {
    use cli::Command;

    match args.command {
        Some(Command::Config(ConfigArgs {
            command: ConfigCommand::Regen,
        })) => {
            // (re)generate the default config
            let file = &args.common.config;
            let provider = AutoDefaultConfigProvider::<config::GeneralConfig>::new(plugins);
            let new_config = provider.default_config()?;
            std::fs::write(file, new_config)?;
            log::info!("Default configuration file written to: {file}");
            Ok(true)
        }
        Some(Command::Plugins(PluginsArgs {
            status: false,
            command: PluginsCommand::List,
        })) => {
            // List available plugins without status.
            println!("Available plugins:");
            for p in plugins.metadata(PluginStatus::Any) {
                println!("- {} v{}", p.name, p.version);
            }
            println!("\nEdit the configuration file or use the --plugins flag to enable/disable plugins.");
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// If selected by the CLI user, runs a command that does not need the measurement pipeline.
///
/// Returns `true` if a command was run (in which case you probably should stop here).
fn run_command_no_measurement(args: &cli::Cli, _config: &GeneralConfig, plugins: &PluginSet) -> anyhow::Result<bool> {
    use cli::Command;

    match args.command {
        Some(Command::Plugins(PluginsArgs {
            status: true,
            command: PluginsCommand::List,
        })) => {
            // List available plugins with enabled/disabled status.
            println!("Enabled plugins:");
            for p in plugins.metadata(PluginStatus::Enabled) {
                println!("- {} v{}", p.name, p.version);
            }
            println!("\nDisabled plugins:");
            for p in plugins.metadata(PluginStatus::Disabled) {
                println!("- {} v{}", p.name, p.version);
            }
            println!("\nEdit the configuration file or use the --plugins flag to enable/disable plugins.");
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn apply_pipeline_settings(args: &cli::Cli, config: &GeneralConfig, pipeline: &mut pipeline::Builder) {
    // config file
    if let Some(max_update_interval) = config.max_update_interval {
        pipeline.trigger_constraints_mut().max_update_interval = max_update_interval.into_inner();
    }
    if let Some(source_channel_size) = config.source_channel_size {
        *pipeline.source_channel_size() = source_channel_size;
    }

    // cli arguments
    if let Some(max_update_interval) = args.common.max_update_interval {
        pipeline.trigger_constraints_mut().max_update_interval = max_update_interval;
    }
    if let Some(source_channel_size) = args.common.source_channel_size {
        *pipeline.source_channel_size() = source_channel_size;
    }
    if matches!(args.command, Some(cli::Command::Exec(_))) {
        // the "exec" command requires event-based source trigger
        pipeline.trigger_constraints_mut().allow_manual_trigger = true;
    }
}

/// Parses the config overrides provided on the command line, and merges them into a single table.
fn parse_config_overrides(args: &cli::Cli) -> anyhow::Result<toml::Table> {
    let mut config_override = toml::Table::new();
    if let Some(overrides) = &args.common.config_override {
        for o in overrides {
            let parsed_override =
                toml::Table::from_str(o).with_context(|| format!("config override is not a valid TOML table: {o}"))?;
            // TODO we could make overrides a bit easier to use by turning
            // `key=value` to `key='value'` automatically (if value is not a number nor boolean)
            merge_override(&mut config_override, parsed_override);
        }
    }
    Ok(config_override)
}

/// Generates a version number from the information generated in the build script.
/// See `build.rs` at the crate root.
fn agent_version() -> String {
    const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");
    if option_env!("ALUMET_AGENT_RELEASE").is_some() {
        const BUILD_DATE: &str = env!("VERGEN_BUILD_DATE");
        format!("{CRATE_VERSION} ({BUILD_DATE})")
    } else {
        let git_hash: &str = option_env!("VERGEN_GIT_SHA").unwrap_or("?");
        const GIT_DIRTY: &str = env!("VERGEN_GIT_DIRTY");
        const BUILD_TIMESTAMP: &str = env!("VERGEN_BUILD_TIMESTAMP");
        const RUSTC_SEMVER: &str = env!("VERGEN_RUSTC_SEMVER");
        const CARGO_DEBUG: &str = env!("VERGEN_CARGO_DEBUG");
        let dirty = if GIT_DIRTY == "true" { "-dirty" } else { "" };
        format!("{CRATE_VERSION}-{git_hash}{dirty} ({BUILD_TIMESTAMP}, rustc {RUSTC_SEMVER}, debug={CARGO_DEBUG})")
    }
}

/// Agent command-line interface (CLI).
///
/// We use `clap` to parse these options, therefore the structs
/// derive [`clap::Args`] or other clap trait implementations.
///
/// To apply "advanced" tweaks, we combine the "derive" and "builder" APIs of clap.
/// See https://docs.rs/clap/latest/clap/_derive/index.html#mixing-builder-and-derive-apis
mod cli {
    use clap::{Args, Parser, Subcommand};
    use std::time::Duration;

    // NOTE: the doc comment attached to `Cli` is used by clap as the description of
    // the application. It is displayed at the start of the help message.

    /// Alumet standard agent: measure energy and performance metrics.
    #[derive(Parser)]
    pub struct Cli {
        #[command(subcommand)]
        pub command: Option<Command>,

        #[command(flatten)]
        pub common: CommonArgs,
    }

    #[derive(Subcommand)]
    pub enum Command {
        /// Run the agent and monitor the system.
        ///
        /// This is the default command.
        Run,

        /// Execute a command and observe its process.
        Exec(ExecArgs),

        /// Manipulate the configuration.
        Config(ConfigArgs),

        /// Get plugins information.
        Plugins(PluginsArgs),
    }

    /// CLI arguments for the `exec` command.
    #[derive(Args)]
    pub struct ExecArgs {
        /// The program to run.
        pub program: String,

        /// Arguments to the program.
        #[arg(trailing_var_arg = true)]
        pub args: Vec<String>,
    }

    #[derive(Args)]
    pub struct ConfigArgs {
        #[command(subcommand)]
        pub command: ConfigCommand,
    }

    #[derive(Subcommand)]
    pub enum ConfigCommand {
        /// Regenerate the configuration file and stop.
        ///
        /// If the file exists, it will be overwritten.
        Regen,
    }

    #[derive(Args)]
    pub struct PluginsArgs {
        // `global=true` adds the flag to every subcommand
        // so you can write `alumet-agent plugins list --status`
        // in addition to `alumet-agent plugins --status list`
        /// Reads the agent config to get the status (enabled/disabled) of each plugin.
        #[arg(long, global = true)]
        pub status: bool,

        #[command(subcommand)]
        pub command: PluginsCommand,
    }

    #[derive(Subcommand)]
    pub enum PluginsCommand {
        /// Print the available plugins.
        List,
    }

    /// Common CLI arguments.
    ///
    /// # Example and tip
    /// Use `#[command(flatten)]` to add these arguments to your args structure.
    ///
    /// See below:
    ///
    /// ```
    /// use clap::Parser;
    /// use alumet_agent::options::cli::CommonArgs;
    ///
    /// #[derive(Parser)]
    /// struct Cli {
    ///     #[command(flatten)]
    ///     common: CommonArgs,
    ///
    ///     my_arg: String,
    /// }
    /// ```
    #[derive(Args, Clone)]
    pub struct CommonArgs {
        /// Path to the config file.
        #[arg(long, env = "ALUMET_CONFIG", default_value = "alumet-config.toml")]
        pub config: String, // not used in Configurator, but directly by main()

        /// If set, the config file must exist, otherwise the agent will fail to start with an error.
        #[arg(long, default_value_t = false)]
        pub no_default_config: bool,

        /// Config options overrides.
        ///
        /// Use dots to separate TOML levels, ex. `plugins.rapl.poll_interval='1ms'`
        #[arg(long)]
        pub config_override: Option<Vec<String>>,

        /// List of plugins to enable, separated by commas, ex. `csv,rapl`.
        ///
        /// All the other plugins will be disabled.
        #[arg(long, value_delimiter = ',')]
        pub plugins: Option<Vec<String>>,

        /// Maximum amount of time between two updates of the sources' commands.
        ///
        /// A lower value means that the latency of source commands will be lower,
        /// i.e. commands will be applied faster, at the cost of a higher overhead.
        #[arg(long, value_parser = humantime_serde::re::humantime::parse_duration)]
        pub max_update_interval: Option<Duration>,

        /// How many `MeasurementBuffer`s can be stored in the channel that sources write to.
        ///
        /// You may want to increase this if you get "buffer is full" errors, which can happen
        /// if you have a large number of sources that flush at the same time.
        #[arg(long)]
        pub source_channel_size: Option<usize>,

        /// How many "normal" worker threads to spawn.
        #[arg(long, env = "ALUMET_NORMAL_THREADS")]
        pub normal_worker_threads: Option<usize>,

        /// How many "high-priority" worker threads to spawn.
        #[arg(long, env = "ALUMET_PRIORITY_THREADS")]
        pub priority_worker_threads: Option<usize>,
    }
}

/// Agent configuration options.
///
/// We use `serde` to parse these options from the TOML config file,
/// and to write the default configuration to the TOML config file,
/// therefore the structs derive [`serde::Deserialize`] and [`serde::Serialize`].
mod config {
    use std::time::Duration;

    use serde::{Deserialize, Serialize};

    /// General config options, which are not specific to a particular plugin.
    #[derive(Deserialize, Serialize, Default)]
    pub struct GeneralConfig {
        pub max_update_interval: Option<humantime_serde::Serde<Duration>>,
        pub source_channel_size: Option<usize>,
    }
}
