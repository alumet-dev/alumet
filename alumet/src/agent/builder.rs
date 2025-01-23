use std::{collections::HashMap, ops::DerefMut, time::Duration};

use anyhow::{anyhow, Context};

use crate::agent::plugin::PluginInfo;
use crate::plugin::phases::PreStartAction;
use crate::plugin::{AlumetPluginStart, AlumetPostStart, ConfigTable, Plugin};
use crate::{
    pipeline::{self, PluginName},
    plugin::{phases::PostStartAction, AlumetPreStart},
};

use super::plugin::PluginSet;

/// An Agent that has been started.
pub struct RunningAgent {
    pub pipeline: pipeline::MeasurementPipeline,
    pub initialized_plugins: Vec<Box<dyn Plugin>>,
}

/// Agent builder.
///
/// # Example
/// ```no_run
/// use alumet::{agent, pipeline, static_plugins};
///
/// struct MyPlugin {}
/// impl alumet::plugin::rust::AlumetPlugin for MyPlugin {
///     // TODO
/// #   fn name() -> &'static str { "" }
/// #   fn version() -> &'static str { "" }
/// #   fn init(config: alumet::plugin::ConfigTable) -> anyhow::Result<Box<Self>> { todo!() }
/// #   fn default_config() -> anyhow::Result<Option<alumet::plugin::ConfigTable>> { todo!() }
/// #   fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> { todo!() }
/// #   fn stop(&mut self) -> anyhow::Result<()> { todo!() }
/// }
///
/// // Get the plugins metadata and configs
/// let plugins = static_plugins![MyPlugin];
/// let mut my_plugin_config: toml::Table = todo!();
///
/// // Create and configure the builder
/// let mut pb = pipeline::Builder::new();
/// let mut builder = agent::Builder::new(pb);
/// builder.add_plugins(plugins);
/// builder.set_plugin_info("my-plugin", true, my_plugin_config);
///
/// // Start Alumet and the plugins
/// let agent = builder.build_and_start();
/// ```
pub struct Builder {
    /// All the plugins (not initialized yet), in order (the order must be preserved).
    plugins: PluginSet,

    /// Builds the measurement pipeline.
    pipeline_builder: pipeline::Builder,

    /// Functions called during the agent startup.
    callbacks: Callbacks,
}

struct Callbacks {
    after_plugins_init: Box<dyn FnOnce(&mut Vec<Box<dyn Plugin>>)>,
    after_plugins_start: Box<dyn FnOnce(&pipeline::Builder)>,
    before_operation_begin: Box<dyn FnOnce(&pipeline::Builder)>,
    after_operation_begin: Box<dyn FnOnce(&mut pipeline::MeasurementPipeline)>,
}

impl Default for Callbacks {
    fn default() -> Self {
        Self {
            after_plugins_init: Box::new(|_| ()),
            after_plugins_start: Box::new(|_| ()),
            before_operation_begin: Box::new(|_| ()),
            after_operation_begin: Box::new(|_| ()),
        }
    }
}

impl Builder {
    /// Creates a new agent builder with a default pipeline.
    pub fn new(plugins: PluginSet) -> Self {
        Self::from_pipeline(plugins, pipeline::Builder::new())
    }

    /// Creates a new agent builder with a custom pipeline.
    ///
    /// Use this when you want to customize the settings of the pipeline.
    /// To get the default pipeline, use [`Builder::new`].
    pub fn from_pipeline(plugins: PluginSet, pipeline_builder: pipeline::Builder) -> Self {
        Self {
            plugins,
            pipeline_builder,
            callbacks: Callbacks::default(),
        }
    }

    /// Sets a function to run after the plugins have been initialized.
    ///
    /// There can be only one callback. If this function is called more than once,
    /// only the last callback will be called.
    pub fn after_plugins_init<F: FnOnce(&mut Vec<Box<dyn Plugin>>) + 'static>(mut self, f: F) -> Self {
        self.callbacks.after_plugins_init = Box::new(f);
        self
    }

    /// Sets a function to run after the plugins have started.
    ///
    /// There can be only one callback. If this function is called more than once,
    /// only the last callback will be called.
    pub fn after_plugins_start<F: FnOnce(&pipeline::Builder) + 'static>(mut self, f: F) -> Self {
        self.callbacks.after_plugins_start = Box::new(f);
        self
    }

    /// Sets a function to run just before the measurement pipeline starts.
    ///
    /// There can be only one callback. If this function is called more than once,
    /// only the last callback will be called.
    pub fn before_operation_begin<F: FnOnce(&pipeline::Builder) + 'static>(mut self, f: F) -> Self {
        self.callbacks.before_operation_begin = Box::new(f);
        self
    }

    /// Sets a function to run after the measurement pipeline has started.
    ///
    /// There can be only one callback. If this function is called more than once,
    /// only the last callback will be called.
    pub fn after_operation_begin<F: FnOnce(&mut pipeline::MeasurementPipeline) + 'static>(mut self, f: F) -> Self {
        self.callbacks.after_operation_begin = Box::new(f);
        self
    }

    /// Builds and starts the underlying measurement pipeline and the enabled plugins.
    pub fn build_and_start(self) -> anyhow::Result<RunningAgent> {
        /// Initializes one plugin.
        ///
        /// Returns the initialized plugin, or an error.
        fn init_plugin(p: PluginInfo) -> anyhow::Result<Box<dyn Plugin>> {
            let name = p.metadata.name;
            let version = p.metadata.version;
            let config = ConfigTable(p.config.unwrap_or_default());
            log::debug!("Initializing plugin {name} v{version} with config {config:?}...");

            // call init
            let initialized = (p.metadata.init)(config)
                .with_context(|| format!("plugin failed to initialize: {} v{}", name, version))?;

            // check that the plugin corresponds to its metadata
            if (initialized.name(), initialized.version()) != (&name, &version) {
                return Err(anyhow!("invalid plugin: metadata is '{name}' v{version} but the plugin's methods return '{name}' v{version}"));
            }
            Ok(initialized)
        }

        /// Starts a plugin, i.e. calls [`Plugin::start`] with the right context.
        fn start_plugin(
            p: &mut dyn Plugin,
            pipeline_builder: &mut pipeline::Builder,
            pre_start_actions: &mut Vec<(pipeline::PluginName, Box<dyn PreStartAction>)>,
            post_start_actions: &mut Vec<(pipeline::PluginName, Box<dyn PostStartAction>)>,
        ) -> anyhow::Result<()> {
            let name = p.name().to_owned();
            let version = p.version().to_owned();
            log::debug!("Starting plugin {name} v{version}...");

            let mut ctx = AlumetPluginStart {
                current_plugin: pipeline::PluginName(name.clone()),
                pipeline_builder,
                pre_start_actions,
                post_start_actions,
            };
            p.start(&mut ctx)
                .with_context(|| format!("plugin failed to start: {name} v{version}"))
        }

        /// Executes the pre-pipeline-start phase of a plugin, i.e. calls [`Plugin::pre_pipeline_start`] with the right context.
        fn pre_pipeline_start(
            p: &mut dyn Plugin,
            pipeline_builder: &mut pipeline::Builder,
            actions: &mut HashMap<PluginName, Vec<Box<dyn PreStartAction>>>,
        ) -> anyhow::Result<()> {
            let name = p.name().to_owned();
            let version = p.version().to_owned();
            log::debug!("Running pre-pipeline-start hook for plugin {name} v{version}...");

            // Prepare the context.
            let pname = pipeline::PluginName(name.clone());
            let mut ctx = AlumetPreStart {
                current_plugin: pname.clone(),
                pipeline_builder,
            };

            // Call pre_pipeline_start.
            p.pre_pipeline_start(&mut ctx)
                .with_context(|| format!("plugin pre_pipeline_start failed: {} v{}", p.name(), p.version()))?;

            // Run the additional actions registered by the plugin, if any.
            if let Some(actions) = actions.remove(&pname) {
                for f in actions {
                    (f)(&mut ctx)
                        .with_context(|| format!("plugin post-pipeline-start action failed: {name} v{version}"))?;
                }
            }
            Ok(())
        }

        /// Executes the post-pipeline-start phase of a plugin, i.e. calls [`Plugin::post_pipeline_start`] with the right context.
        ///
        /// Plugins can also register post-pipeline-start actions in the form of closures, we run these too.
        fn post_pipeline_start(
            p: &mut dyn Plugin,
            pipeline: &mut pipeline::MeasurementPipeline,
            actions: &mut HashMap<PluginName, Vec<Box<dyn PostStartAction>>>,
        ) -> anyhow::Result<()> {
            let name = p.name().to_owned();
            let version = p.version().to_owned();
            log::debug!("Running post-pipeline-start hook for plugin {name} v{version}...");

            // Prepare the context.
            let pname = pipeline::PluginName(name.clone());
            let mut ctx = AlumetPostStart {
                current_plugin: pname.clone(),
                pipeline,
            };

            // Call post_pipeline_start.
            p.post_pipeline_start(&mut ctx)
                .with_context(|| format!("plugin post_pipeline_start method failed: {name} v{version}"))?;

            // Run the additional actions registered by the plugin, if any.
            if let Some(actions) = actions.remove(&pname) {
                for f in actions {
                    (f)(&mut ctx)
                        .with_context(|| format!("plugin post-pipeline-start action failed: {name} v{version}"))?;
                }
            }
            Ok(())
        }

        /// Groups all pre or post-start actions by plugin.
        fn group_plugin_actions<BoxedAction>(
            post_start_actions: Vec<(PluginName, BoxedAction)>,
            n_plugins: usize,
        ) -> HashMap<PluginName, Vec<BoxedAction>> {
            let mut res = HashMap::with_capacity(n_plugins);
            for (plugin, action) in post_start_actions {
                let plugin_actions: &mut Vec<_> = res.entry(plugin).or_default();
                plugin_actions.push(action);
            }
            res
        }

        // Find which plugins are enabled.
        log::info!("Initializing the plugins...");
        let (enabled_plugins, disabled_plugins): (Vec<PluginInfo>, Vec<PluginInfo>) = self.plugins.into_partition();

        // Initialize the plugins that are enabled.
        let initialized_plugins: anyhow::Result<Vec<Box<dyn Plugin>>> =
            enabled_plugins.into_iter().map(init_plugin).collect();
        let mut initialized_plugins = initialized_plugins?;
        let n_plugins = initialized_plugins.len();
        match n_plugins {
            0 if disabled_plugins.is_empty() => log::warn!("No plugin has been initialized, there may be a problem with your agent implementation. Please check your builder."),
            0 => log::warn!("No plugin has been initialized because they were all disabled in the config. Please check your configuration."),
            1 => log::info!("1 plugin initialized."),
            n => log::info!("{n} plugins initialized."),
        };
        (self.callbacks.after_plugins_init)(&mut initialized_plugins);

        // Start-up phase.
        log::info!("Starting the plugins...");
        let mut pipeline_builder = self.pipeline_builder;
        let mut pre_start_actions = Vec::new();
        let mut post_start_actions = Vec::new();
        for plugin in initialized_plugins.iter_mut() {
            start_plugin(
                plugin.deref_mut(),
                &mut pipeline_builder,
                &mut pre_start_actions,
                &mut post_start_actions,
            )?;
        }
        print_stats(&pipeline_builder, &initialized_plugins, &disabled_plugins);
        (self.callbacks.after_plugins_start)(&pipeline_builder);

        // pre-pipeline-start actions
        log::info!("Running pre-pipeline-start hooks...");
        let mut pre_actions_per_plugin = group_plugin_actions(pre_start_actions, n_plugins);
        for plugin in initialized_plugins.iter_mut() {
            pre_pipeline_start(plugin.deref_mut(), &mut pipeline_builder, &mut pre_actions_per_plugin)?;
        }
        (self.callbacks.before_operation_begin)(&pipeline_builder);

        // Build and start the pipeline.
        log::info!("Starting the measurement pipeline...");
        let mut pipeline = pipeline_builder.build().context("Pipeline failed to build")?;
        log::info!("🔥 ALUMET measurement pipeline has started.");

        // post-pipeline-start actions
        log::info!("Running post-pipeline-start hooks...");
        let mut post_actions_per_plugin = group_plugin_actions(post_start_actions, n_plugins);
        for plugin in initialized_plugins.iter_mut() {
            post_pipeline_start(plugin.deref_mut(), &mut pipeline, &mut post_actions_per_plugin)?;
        }
        (self.callbacks.after_operation_begin)(&mut pipeline);

        log::info!("🔥 ALUMET agent is ready.");

        let agent = RunningAgent {
            pipeline,
            initialized_plugins,
        };
        Ok(agent)
    }
}

impl RunningAgent {
    /// Waits until the measurement pipeline stops, then stops the plugins.
    ///
    /// See the [module documentation](super::agent).
    pub fn wait_for_shutdown(self, timeout: Duration) -> anyhow::Result<()> {
        use std::panic::{catch_unwind, AssertUnwindSafe};
        let mut n_errors = 0;

        // Tokio's timeout has a maximum timeout that is much smaller than Duration::MAX,
        // and will replace the latter by its maximum timeout.
        // Therefore, we use an Option to disable the timeout if it's Duration::MAX.
        let timeout = Some(timeout).filter(|d| *d != Duration::MAX);

        // Wait for the pipeline to be stopped, by Ctrl+C or a command.
        // Also, **drop** the pipeline before stopping the plugin, because Plugin::stop expects
        // the sources, transforms and outputs to be stopped and dropped before it is called.
        // All tokio tasks that have not finished yet will abort.
        match self.pipeline.wait_for_shutdown(timeout) {
            Ok(Ok(_)) => (),
            Ok(Err(err)) => {
                log::error!("Error in the measurement pipeline: {err:?}");
                n_errors += 1;
            }
            Err(_elapsed) => {
                log::error!(
                    "Timeout of {:?} expired while waiting for the pipeline to shut down",
                    timeout.unwrap()
                );
                n_errors += 1;
            }
        }

        // Stop all the plugins, even if some of them fail to stop properly.
        log::info!("Stopping the plugins...");
        for mut plugin in self.initialized_plugins {
            let name = plugin.name().to_owned();
            let version = plugin.version().to_owned();
            log::info!("Stopping plugin {name} v{version}");

            // If a plugin panics, we still want to try to stop the other plugins.
            match catch_unwind(AssertUnwindSafe(move || {
                plugin.stop()
                // plugin is dropped here
            })) {
                Ok(Ok(())) => (),
                Ok(Err(e)) => {
                    log::error!("Error while stopping plugin {name} v{version}. {e:#}");
                    n_errors += 1;
                }
                Err(panic_payload) => {
                    log::error!(
                        "PANIC while stopping plugin {name} v{version}. There is probably a bug in the plugin!
                        Please check the implementation of stop (and drop if Drop is implemented for the plugin type)."
                    );
                    n_errors += 1;
                    // dropping the panic payload may, in turn, panic!
                    let _ = catch_unwind(AssertUnwindSafe(move || {
                        drop(panic_payload);
                    }))
                    .map_err(|panic2| {
                        log::error!(
                            "PANIC while dropping panic payload generated while stopping plugin {name} v{version}."
                        );
                        // We cannot drop it, forget it.
                        // Alumet will stop after this anyway, but the plugin should be fixed.
                        std::mem::forget(panic2);
                    });
                }
            }
        }
        log::info!("All plugins have stopped.");

        if n_errors == 0 {
            Ok(())
        } else {
            let error_str = if n_errors == 1 { "error" } else { "errors" };
            Err(anyhow!("{n_errors} {error_str} occurred during the shutdown phase"))
        }
    }
}

/// Prints some statistics after the plugin start-up phase.
fn print_stats(
    pipeline_builder: &pipeline::Builder,
    enabled_plugins: &[Box<dyn Plugin>],
    disabled_plugins: &[PluginInfo],
) {
    macro_rules! pluralize {
        ($count:expr, $str:expr) => {
            if $count > 1 {
                concat!($str, "s")
            } else {
                $str
            }
        };
    }

    // format plugin lists
    let enabled_list: String = enabled_plugins
        .iter()
        .map(|p| format!("    - {} v{}", p.name(), p.version()))
        .collect::<Vec<_>>()
        .join("\n");
    let disabled_list: String = disabled_plugins
        .iter()
        .map(|p| format!("    - {} v {}", p.metadata.name, p.metadata.version))
        .collect::<Vec<_>>()
        .join("\n");
    let n_enabled = enabled_plugins.len();
    let n_disabled = disabled_plugins.len();
    let enabled_str = pluralize!(n_enabled, "plugin");
    let disabled_str = pluralize!(n_disabled, "plugin");

    // format metric list
    let metrics = &pipeline_builder.metrics;
    let metric_list = if metrics.is_empty() {
        String::from("    ∅")
    } else {
        let mut m = metrics
            .iter()
            .map(|(id, m)| (id, format!("    - {}: {} ({})", m.name, m.value_type, m.unit)))
            .collect::<Vec<_>>();
        // Sort by metric id to display the metrics in the order they were registered (less confusing).
        m.sort_by_key(|(id, _)| id.0);
        m.into_iter()
            .map(|(_, metric_str)| metric_str)
            .collect::<Vec<_>>()
            .join("\n")
    };

    // format pipeline statistics
    let stats = pipeline_builder.stats();

    let n_sources = stats.sources;
    let n_transforms = stats.transforms;
    let n_outputs = stats.outputs;
    let n_metric_listeners = stats.metric_listeners;

    let source_str = pluralize!(n_sources, "source");
    let transform_str = pluralize!(n_transforms, "transform");
    let output_str = pluralize!(n_outputs, "output");
    let metric_listener_str = pluralize!(n_metric_listeners, "metric listener");

    let n_metrics = stats.metrics;
    let str_metric = pluralize!(n_metrics, "metric");
    let msg = indoc::formatdoc! {"
        Plugin startup complete.
        🧩 {n_enabled} {enabled_str} started:
        {enabled_list}
        
        ⭕ {n_disabled} {disabled_str} disabled:
        {disabled_list}
        
        📏 {n_metrics} {str_metric} registered:
        {metric_list}
        
        📥 {n_sources} {source_str}, 🔀 {n_transforms} {transform_str} and 📝 {n_outputs} {output_str} registered.
        
        🔔 {n_metric_listeners} {metric_listener_str} registered.
        "
    };
    log::info!("{msg}");
}
