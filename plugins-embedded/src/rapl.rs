use alumet_api::plugin::Plugin;


struct RaplPlugin;

impl Plugin for RaplPlugin {
    fn name(&self) -> &str {
        "rapl"
    }

    fn version(&self) -> &str {
        "0.0.1"
    }

    fn start(&mut self, metrics: &mut alumet_api::metric::MetricRegistry, sources: &mut alumet_api::plugin::SourceRegistry, outputs: &mut alumet_api::plugin::OutputRegistry) -> alumet_api::plugin::PluginResult<()> {
        todo!()
    }

    fn stop(&mut self) -> alumet_api::plugin::PluginResult<()> {
        todo!()
    }
}
