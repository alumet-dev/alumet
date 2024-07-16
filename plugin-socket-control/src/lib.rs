mod command;
mod socket;

use alumet::{
    pipeline::runtime::RunningPipeline,
    plugin::{
        rust::{deserialize_config, serialize_config, AlumetPlugin},
        AlumetStart, ConfigTable,
    },
};
use serde::{Deserialize, Serialize};
use socket::SocketControl;

#[derive(Deserialize, Serialize)]
pub struct Config {
    socket_path: String,
}

pub struct SocketControlPlugin {
    config: Config,
    control: Option<SocketControl>,
}

impl AlumetPlugin for SocketControlPlugin {
    fn name() -> &'static str {
        "socket-control"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        let config = serialize_config(Config::default())?;
        Ok(Some(config))
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config)?;
        Ok(Box::new(SocketControlPlugin {
            config: config,
            control: None,
        }))
    }

    fn start(&mut self, _alumet: &mut AlumetStart) -> anyhow::Result<()> {
        Ok(())
    }

    fn post_pipeline_start(&mut self, pipeline: &mut RunningPipeline) -> anyhow::Result<()> {
        // Enable remote control via Unix socket.
        let control = SocketControl::start_new(pipeline.control_handle(), &self.config.socket_path)?;
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
            let _ = std::fs::remove_file(&self.config.socket_path);
        }
        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            socket_path: String::from("alumet-control.sock"),
        }
    }
}
