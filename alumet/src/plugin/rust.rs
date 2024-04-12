use crate::{
    config::ConfigTable,
    plugin::{AlumetStart, Plugin},
};

/// Trait for Alumet plugins written in Rust.
///
/// Implement this trait to define your plugin.
pub trait AlumetPlugin {
    // Note: add `where Self: Sized` to make this trait "object safe", if necessary in the future.

    fn name() -> &'static str;
    fn version() -> &'static str;

    /// Initializes the plugin.
    ///
    /// Read more about the plugin lifecycle in the [module documentation](super).
    fn init(config: &mut ConfigTable) -> anyhow::Result<Box<Self>>;

    /// Starts the plugin, allowing it to register metrics, sources and outputs.
    ///
    /// ## Plugin restart
    /// A plugin can be started and stopped multiple times, for instance when ALUMET switches from monitoring to profiling mode.
    /// [`Plugin::stop`] is guaranteed to be called between two calls of [`Plugin::start`].
    fn start(&mut self, alumet: &mut AlumetStart) -> anyhow::Result<()>;

    /// Stops the plugin.
    ///
    /// This method is called _after_ all the metrics, sources and outputs previously registered
    /// by [`Plugin::start`] have been stopped and unregistered.
    fn stop(&mut self) -> anyhow::Result<()>;
}

// Every AlumetPlugin is a Plugin :)
impl<P: AlumetPlugin> Plugin for P {
    fn name(&self) -> &str {
        P::name() as _
    }

    fn version(&self) -> &str {
        P::version() as _
    }

    fn start(&mut self, alumet: &mut AlumetStart) -> anyhow::Result<()> {
        AlumetPlugin::start(self, alumet)
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        AlumetPlugin::stop(self)
    }
}
