mod command;
mod socket;

use alumet::{
    pipeline::runtime::RunningPipeline,
    plugin::{rust::AlumetPlugin, AlumetStart, ConfigTable},
};
use socket::SocketControl;

pub struct SocketControlPlugin {
    control: Option<SocketControl>,
    socket_path: String,
}

impl AlumetPlugin for SocketControlPlugin {
    fn name() -> &'static str {
        env!("CARGO_PKG_NAME")
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        // TODO config options
        Ok(Box::new(SocketControlPlugin { control: None, socket_path: String::from("alumet-control.sock") }))
    }

    fn start(&mut self, _alumet: &mut AlumetStart) -> anyhow::Result<()> {
        Ok(())
    }

    fn post_pipeline_start(&mut self, pipeline: &mut RunningPipeline) -> anyhow::Result<()> {
        // Enable remote control via Unix socket.
        let control = SocketControl::start_new(pipeline.control_handle(), &self.socket_path)?;
        self.control = Some(control);
        log::info!("SocketControl enabled.");
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        if let Some(control) = self.control.take() {
            control.stop();
            control.join();
            log::info!("SocketControl stopped.");

            // delete the socket file
            let _ = std::fs::remove_file(&self.socket_path);
        }
        Ok(())
    }
}
