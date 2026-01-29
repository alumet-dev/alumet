use std::{process::ExitCode, str::FromStr, time::Duration};

use alumet::{
    agent::{
        self,
        config::{AutoDefaultConfigProvider, DefaultConfigProvider, NoDefaultConfigProvider, merge_override},
        exec,
        plugin::{PluginFilter, PluginSet, UnknownPluginInConfigPolicy},
        watch,
    },
    pipeline,
    plugin::PluginMetadata,
    static_plugins,
};
use alumet_agent::{exec_hints, init_logger};
use anyhow::Context;
use clap::{Args, FromArgMatches};
use cli::{ConfigArgs, ConfigCommand, PluginsArgs, PluginsCommand};
use config::GeneralConfig;

const BINARY: &str = env!("CARGO_BIN_NAME");

/// Loads the available plugins.
fn load_plugins_metadata() -> Vec<PluginMetadata> {
    // plugins that work on every target
    let mut plugins = static_plugins![
        plugin_csv::CsvPlugin,
        plugin_prometheus_exporter::PrometheusPlugin,
        plugin_influxdb::InfluxDbPlugin,
        plugin_mongodb::MongoDbPlugin,
        plugin_relay::client::RelayClientPlugin,
        plugin_relay::server::RelayServerPlugin,
        plugin_opentelemetry::OpenTelemetryPlugin,
        plugin_aggregation::AggregationPlugin,
        plugin_energy_attribution::EnergyAttributionPlugin,
        plugin_energy_estimation_tdp::EnergyEstimationTdpPlugin,
        plugin_elasticsearch::ElasticSearchPlugin,
        plugin_kwollect_input::KwollectPluginInput,
        plugin_kwollect_output::KwollectPlugin,
    ];

    // plugins that only work on Linux
    #[cfg(target_os = "linux")]
    {
        plugins.extend(static_plugins![
            plugin_socket_control::SocketControlPlugin,
            plugin_k8s::K8sPlugin,
            plugin_slurm::SlurmPlugin,
            plugin_oar::OarPlugin,
            plugin_raw_cgroups::RawCgroupPlugin,
            plugin_grace_hopper::GraceHopperPlugin,
            plugin_rapl::RaplPlugin,
            plugin_perf::PerfPlugin,
            plugin_procfs::ProcfsPlugin,
            plugin_nvidia_nvml::NvmlPlugin,
            plugin_process_to_cgroup_bridge::ProcessToCgroupBridgePlugin,
            plugin_nvidia_jetson::JetsonPlugin,
            plugin_quarch::QuarchPlugin,
        ]);
    }

    plugins
}

/// Main agent function.
///
/// The steps are:
/// - load the metadata of the available plugins
/// - parse the CLI
/// - parse the config file
/// - apply the settings from CLI and config file
/// - start the Alumet plugins and pipeline
/// - wait for the stop condition
///
/// About errors: we use `anyhow::Result` and `context` instead of `expect` to get
/// nicer error messages (`expect` prints errors with `Debug`).
fn main() -> anyhow::Result<ExitCode> {
    init_logger();

    // Load plugins metadata.
    let mut plugins = PluginSet::from(load_plugins_metadata());

    // Define the command-line interface.
    let mut cmd = clap::Command::new(BINARY).version(agent_version());
    cmd = cli::Cli::augment_args(cmd);

    // Parse CLI arguments and handle some special flags like --version and --help.
    let matches = cmd.get_matches();
    let mut args = cli::Cli::from_arg_matches(&matches).map_err(|e| e.exit()).unwrap();

    // Special flags like --help will exit. In other cases, we continue.
    print_welcome();

    // If the CLI args override the list of enabled plugins, we need to know it now,
    // because that will change how some "no config" commands work (such as config regen).
    if let Some(enabled_plugins) = &args.common.plugins {
        plugins.enable_only(enabled_plugins);
    }

    // Run CLI commands that run before the config is loaded.
    if run_command_no_config(&args, &plugins)? {
        return Ok(ExitCode::SUCCESS);
    }

    // parse config file
    let config_override = parse_config_overrides(&args).context("invalid config overrides")?;
    let default_config_provider: Box<dyn DefaultConfigProvider> = if args.common.no_default_config {
        Box::new(NoDefaultConfigProvider)
    } else {
        Box::new(AutoDefaultConfigProvider::new(&plugins, config::GeneralConfig::default))
    };
    let mut config = agent::config::Loader::parse_file(&args.common.config)
        .or_default_boxed(default_config_provider, true)
        .substitute_env_variables(true)
        .with_override(config_override)
        .load()
        .context("could not load config file")?;

    // Extract the config of each plugin.
    // If not set by CLI args, use the config to determine which plugins are enabled.
    let plugins_config_order = plugins
        .extract_config(
            &mut config,
            args.common.plugins.is_none(),
            UnknownPluginInConfigPolicy::Error,
        )
        .context("invalid plugins config")?;

    // Extract non-plugin config.
    let config = config.try_into::<GeneralConfig>().context("invalid general config")?;

    // Reorder the plugins according to the configuration.
    plugins.reorder_partial(&plugins_config_order);

    // Run CLI commands that only require the config and run before the pipeline starts.
    if run_command_no_measurement(&args, &config, &plugins).context("command failed")? {
        return Ok(ExitCode::SUCCESS);
    }

    // begin the creation of the pipeline (we have some settings to apply to it)
    let mut pipeline = pipeline::Builder::new();
    apply_pipeline_settings(&args, &config, &mut pipeline);

    // start Alumet with the pipeline and plugins
    let agent = agent::Builder::from_pipeline(plugins, pipeline)
        .build_and_start()
        .context("startup failure")?;

    // run the provided command, the default is Run
    match args.command.take().unwrap_or(cli::Command::Run) {
        cli::Command::Run => {
            // execute the pipeline until Alumet is externally stopped (e.g. by Ctrl+C)
            agent.wait_for_shutdown(Duration::MAX).context("error while running")?;
        }
        cli::Command::Exec(exec_args) => {
            let timeout = Duration::from_secs(5);
            let res = exec::exec_process(agent, exec_args.program, exec_args.args, timeout);
            match &res {
                Ok(_) if exec_args.ignore_exit_code => (),
                Ok(process_exit_code) => {
                    // Attempt to propagate the exit code, if possible.
                    if process_exit_code.success() {
                        return Ok(ExitCode::SUCCESS);
                    } else if let Some(code) = process_exit_code.code().and_then(|i| u8::try_from(i).ok()) {
                        log::warn!(
                            "Propagating failure exit code from the child process. Use `exec --ignore-exit-code` to disable the propagation."
                        );
                        return Ok(ExitCode::from(code));
                    } else {
                        log::warn!(
                            "Child process exited with {process_exit_code}, which cannot be propagated. Returning ExitCode::FAILURE"
                        );
                        return Ok(ExitCode::FAILURE);
                    }
                }
                Err(err @ exec::ExecError::ProcessSpawn(program, e)) => {
                    // print some helpful hints for common problems
                    match e.kind() {
                        std::io::ErrorKind::NotFound => {
                            panic!("{}", exec_hints::handle_not_found(program.clone(), Vec::new()));
                        }
                        std::io::ErrorKind::PermissionDenied => {
                            panic!("{}", exec_hints::handle_permission_denied(program.clone()));
                        }
                        _ => {
                            panic!("{err}");
                        }
                    }
                }
                Err(err) => panic!("{err}"),
            }
        }
        cli::Command::Watch(process) => {
            let shutdown_timeout = Duration::from_secs(5);
            let res = watch::watch_process(agent, process.pid, shutdown_timeout);

            if let Err(watch::WatchError::ProcessWait(pid, e)) = &res
                && e.kind() == std::io::ErrorKind::NotFound
            {
                // common problem: the process does not exist
                panic!("PID: {pid} seems to be not found, error: {e}");
            } else if let Err(err) = res {
                panic!("{err}");
            }
        }
        _ => unreachable!("every command should have been handled at this point"),
    }
    Ok(ExitCode::SUCCESS)
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
            let provider = AutoDefaultConfigProvider::new(plugins, config::GeneralConfig::default);
            let new_config = provider.default_config_string()?;
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
            for p in plugins.metadata(PluginFilter::Any) {
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
            for p in plugins.metadata(PluginFilter::Enabled) {
                println!("- {} v{}", p.name, p.version);
            }
            println!("\nDisabled plugins:");
            for p in plugins.metadata(PluginFilter::Disabled) {
                println!("- {} v{}", p.name, p.version);
            }
            println!("\nEdit the configuration file or use the --plugins flag to enable/disable plugins.");
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// Setup the measurement pipeline according to CLI args and config file.
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

    // Special case `--output` for easier local use.
    if let Some(output) = &args.common.output_file {
        let o = plugin_config_override("csv", "output_path", toml::Value::String(output.to_owned()));
        merge_override(&mut config_override, o);
    }

    // Special case `--relay-out`.
    if let Some(addr) = &args.common.relay_out {
        let o = plugin_config_override("relay-client", "relay_server", toml::Value::String(addr.to_owned()));
        merge_override(&mut config_override, o);
    }

    // Special case `--relay-in`.
    if let Some(addr) = &args.common.relay_in {
        let o = plugin_config_override("relay-server", "address", toml::Value::String(addr.to_owned()));
        merge_override(&mut config_override, o);
    }

    Ok(config_override)
}

/// Generates a table `plugins.csv.output_path = $output`
fn plugin_config_override(plugin: &str, key: &str, value: toml::Value) -> toml::Table {
    toml::Table::from_iter(vec![(
        String::from("plugins"),
        toml::Value::Table(toml::Table::from_iter(vec![(
            String::from(plugin),
            toml::Value::Table(toml::Table::from_iter(vec![(String::from(key), value)])),
        )])),
    )])
}

/// Generates a version number from the information generated in the build script.
/// See `build.rs` at the crate root.
fn agent_version() -> String {
    fn no_vergen_default(s: &&str) -> bool {
        *s != "VERGEN_IDEMPOTENT_OUTPUT"
    }

    const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");
    if option_env!("ALUMET_AGENT_RELEASE").is_some() {
        let build_date: &str = env!("VERGEN_BUILD_DATE");
        format!("{CRATE_VERSION} ({build_date})")
    } else {
        let git_hash: &str = option_env!("VERGEN_GIT_SHA")
            .filter(no_vergen_default)
            .unwrap_or("alpha-unknown");
        let git_dirty: &str = option_env!("VERGEN_GIT_DIRTY")
            .filter(no_vergen_default)
            .unwrap_or("dirty?");
        const BUILD_TIMESTAMP: &str = env!("VERGEN_BUILD_TIMESTAMP");
        const RUSTC_SEMVER: &str = env!("VERGEN_RUSTC_SEMVER");
        const CARGO_DEBUG: &str = env!("VERGEN_CARGO_DEBUG");
        let dirty = if git_dirty == "true" { "-dirty" } else { "" };
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

        /// Watch a PID and observe it until its end
        Watch(Process),

        /// Manipulate the configuration.
        Config(ConfigArgs),

        /// Get plugins information.
        Plugins(PluginsArgs),
    }

    /// CLI arguments for the `exec` command.
    #[derive(Args)]
    pub struct ExecArgs {
        /// If set, don't propagate the exit code of the executed program.
        ///
        /// Note that, if the executed program succeeds but the Alumet agent fails,
        /// the exit code of the agent will always indicate a failure, whether this
        /// flag is set or not.
        #[arg(long, default_value_t = false)]
        pub ignore_exit_code: bool,

        /// The program to run.
        pub program: String,

        /// Arguments to the program.
        #[arg(trailing_var_arg = true)]
        pub args: Vec<String>,
    }

    /// CLI arguments for the `watch` command.
    #[derive(Args)]
    pub struct Process {
        /// The PID to watch.
        pub pid: u32,
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
        pub config: String,

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

        /// Path to the output file (CSV plugin).
        #[arg(long)]
        pub output_file: Option<String>,

        /// Address and port of the server to connect to with the relay client (relay-client plugin).
        #[arg(long)]
        pub relay_out: Option<String>,

        /// Address and/or port that the relay server should listen to (relay-server plugin).
        #[arg(long)]
        pub relay_in: Option<String>,
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
        // TODO move these to an "advanced" table
        pub max_update_interval: Option<humantime_serde::Serde<Duration>>,
        pub source_channel_size: Option<usize>,
    }
}
