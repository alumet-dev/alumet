mod socket;

use alumet::{
    config::ConfigTable,
    pipeline::runtime::RunningPipeline,
    plugin::{rust::AlumetPlugin, AlumetStart},
};
use socket::SocketControl;

pub struct SocketControlPlugin {
    control: Option<SocketControl>,
}

impl AlumetPlugin for SocketControlPlugin {
    fn name() -> &'static str {
        env!("CARGO_PKG_NAME")
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn init(config: &mut ConfigTable) -> anyhow::Result<Box<Self>> {
        // TODO config options
        Ok(Box::new(SocketControlPlugin { control: None }))
    }

    fn start(&mut self, alumet: &mut AlumetStart) -> anyhow::Result<()> {
        Ok(())
    }

    fn post_pipeline_start(&mut self, pipeline: &mut RunningPipeline) -> anyhow::Result<()> {
        // Enable remote control via Unix socket.
        let control = SocketControl::start_new(pipeline.control_handle()).expect("Control thread failed to start");
        self.control = Some(control);
        log::info!("SocketControl enabled.");
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        if let Some(control) = self.control.take() {
            log::info!("Stopping SocketControl...");
            control.stop();
            control.join();
            log::info!("SocketControl stopped.");
        }
        Ok(())
    }
}
