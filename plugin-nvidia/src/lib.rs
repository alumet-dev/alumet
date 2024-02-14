mod nvml;
mod jetson;

struct NvidiaPlugin;

impl alumet::plugin::Plugin for NvidiaPlugin {
    fn name(&self) -> &str {
        "nvidia"
    }

    fn version(&self) -> &str {
        "0.1.0"
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetStart) -> anyhow::Result<()> {
        todo!()
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        todo!()
    }
}
