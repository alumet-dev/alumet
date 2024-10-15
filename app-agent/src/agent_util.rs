//! Agent commands.

use std::{collections::HashMap, path::PathBuf, time::Duration};

use alumet::{
    agent,
    plugin::{rust::InvalidConfig, PluginMetadata},
};
use anyhow::{anyhow, Context};
use serde::{Deserialize, Serialize};

use crate::{
    exec_process,
    options::{Configurator, ContextDefault},
    relative_app_path_string,
};

pub struct ConfigLoadOptions<'a> {
    pub config_path: PathBuf,
    pub plugins: &'a [PluginMetadata],
    pub config_override: Option<toml::Table>,
    pub on_missing_config: MissingConfigStrategy,
}

pub enum MissingConfigStrategy {
    GenerateDefault,
    ReturnError,
}

impl<'a> ConfigLoadOptions<'a> {
    pub fn new(cli_args: &mut super::options::cli::CommonArgs, plugins: &'a [PluginMetadata]) -> anyhow::Result<Self> {
        let config_override = cli_args.take_config_override_table(plugins)?;
        let on_missing_config = if cli_args.no_default_config {
            MissingConfigStrategy::ReturnError
        } else {
            MissingConfigStrategy::GenerateDefault
        };
        Ok(Self {
            config_path: PathBuf::from(cli_args.config.clone()),
            plugins,
            config_override,
            on_missing_config,
        })
    }
}

pub fn default_config(plugins: &[PluginMetadata], additional: toml::Table) -> anyhow::Result<toml::Table> {
    let mut config = toml::Table::new();
    alumet::agent::config::insert_default_plugin_configs(plugins, &mut config)?;
    super::config_ops::merge_override(&mut config, additional);
    Ok(config)
}

pub fn load_config<'de, C: Deserialize<'de> + Serialize + ContextDefault>(
    options: ConfigLoadOptions<'_>,
) -> anyhow::Result<(C, HashMap<String, (bool, toml::Table)>)> {
    let plugins = options.plugins;
    let config_path = &options.config_path;

    // parse the file or generate a default one
    let generate_default = || match options.on_missing_config {
        MissingConfigStrategy::ReturnError => Err(anyhow!("missing config file: {config_path:?}")),
        MissingConfigStrategy::GenerateDefault => {
            let additional = toml::Table::try_from(C::default_with_context(plugins))?;
            default_config(plugins, additional)
        }
    };
    let mut config = alumet::agent::config::parse_file_with_default(config_path, generate_default)?;

    // override some values
    if let Some(overrider) = options.config_override {
        super::config_ops::merge_override(&mut config, overrider);
    }

    // separate plugin configs from the rest of the config
    let plugin_configs = alumet::agent::config::extract_plugin_configs(&mut config)?;
    let non_plugin_config = toml::Value::Table(config).try_into::<C>()?;

    Ok((non_plugin_config, plugin_configs))
}

pub fn new_agent(c1: impl Configurator, c2: impl Configurator, c3: impl Configurator) -> alumet::agent::Builder {
    let mut configurators: Vec<Box<dyn Configurator>> = vec![Box::new(c1), Box::new(c2), Box::new(c3)];
    new_configured_agent(&mut configurators)
}

pub fn new_configured_agent<'a>(configurators: &mut [Box<dyn Configurator + 'a>]) -> alumet::agent::Builder {
    let mut pipeline_builder = alumet::pipeline::Builder::new();
    for c in configurators.iter_mut() {
        c.configure_pipeline(&mut pipeline_builder);
    }

    let mut agent_builder = alumet::agent::Builder::new(pipeline_builder);
    for c in configurators.iter_mut() {
        c.configure_agent(&mut agent_builder);
    }

    agent_builder
}

pub struct PluginsInfo {
    plugins: Vec<PluginMetadata>,
    plugin_configs: HashMap<String, (bool, toml::Table)>,
}

impl PluginsInfo {
    pub fn new(plugins: Vec<PluginMetadata>, plugin_configs: HashMap<String, (bool, toml::Table)>) -> Self {
        Self {
            plugins,
            plugin_configs,
        }
    }
}

impl Configurator for PluginsInfo {
    fn configure_agent(&mut self, agent: &mut alumet::agent::Builder) {
        let plugins = std::mem::take(&mut self.plugins);
        let infos = std::mem::take(&mut self.plugin_configs);
        log::trace!("Adding plugins: {plugins:?}");
        agent.add_plugins(plugins);

        for (plugin, (enabled, config)) in infos {
            log::trace!("set_plugin_info: {plugin} enabled={enabled} config={config:?}");
            agent.set_plugin_info(&plugin, enabled, config);
        }
    }
}

/// Builds and starts the Alumet agent, and handle errors automatically.
pub fn start(agent_builder: agent::Builder) -> agent::RunningAgent {
    agent_builder.build_and_start().unwrap_or_else(|err| {
        log::error!("{err:?}");
        if err.downcast_ref::<InvalidConfig>().is_some() {
            hint_regen_config();
        }
        panic!("ALUMET agent failed to start: {err}");
    })
}

pub fn regen_config<'a, C: Serialize + ContextDefault>(options: ConfigLoadOptions<'a>) -> anyhow::Result<()> {
    let additional = toml::Table::try_from(C::default_with_context(options.plugins))?;
    let mut config = default_config(options.plugins, additional)?;

    // overrides some values if asked to
    if let Some(overrider) = options.config_override {
        super::config_ops::merge_override(&mut config, overrider);
    }

    let config_path = &options.config_path;
    std::fs::write(config_path, config.to_string())
        .with_context(|| format!("failed to write the default configuration to {config_path:?}"))?;

    log::info!("Configuration file (re)generated to: {}", config_path.display());

    Ok(())
}

/// Keeps the agent running until the program stops.
pub fn run_until_stop(agent: alumet::agent::RunningAgent) {
    agent.wait_for_shutdown(Duration::MAX).unwrap();
}

/// Executes a process and stops the agent when the process exits.
pub fn exec_process(agent: alumet::agent::RunningAgent, program: String, args: Vec<String>) {
    // At least one measurement.
    if let Err(e) = exec_process::trigger_measurement_now(&agent.pipeline) {
        log::error!("Could not trigger one last measurement after the child's exit: {e}");
    }

    // Spawn the process and wait for it to exit.
    let exit_status = exec_process::exec_child(program, args).expect("the child should be waitable");
    log::info!("Child process exited with status {exit_status}, Alumet will now stop.");

    // One last measurement.
    if let Err(e) = exec_process::trigger_measurement_now(&agent.pipeline) {
        log::error!("Could not trigger one last measurement after the child's exit: {e}");
    }

    // Stop the pipeline
    agent.pipeline.control_handle().shutdown();
    agent.wait_for_shutdown(Duration::MAX).unwrap();
}

fn hint_regen_config() {
    let exe_path = relative_app_path_string();
    log::error!("HINT: You could try to regenerate the configuration by running `'{}' regen-config` (use --help to get more information).", exe_path.display());
}
