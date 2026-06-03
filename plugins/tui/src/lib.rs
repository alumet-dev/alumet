//! Displays Alumet measurements in an interactive, `htop`-style terminal UI (see [`tui`]).
//!
//! The UI keeps the latest value of every series and lets you filter/sort live, plus open real-time
//! graph tabs. It requires an interactive terminal on stdout; when stdout is not a terminal (e.g.
//! piped to a file), the UI is not displayed.

mod logcap;
mod logo;
mod model;
mod output;
mod theme;
mod tui;

use std::io::IsTerminal;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use alumet::plugin::{
    ConfigTable,
    rust::{AlumetPlugin, deserialize_config, serialize_config},
};
use serde::{Deserialize, Serialize};

use crate::model::Model;
use crate::output::{TuiOutput, TuiOutputSettings};

pub struct TuiPlugin {
    config: Config,
    /// Set to `true` to ask the UI thread to stop.
    stop: Arc<AtomicBool>,
    /// Handle of the interactive UI thread, if one was spawned.
    ui_thread: Option<JoinHandle<()>>,
}

impl AlumetPlugin for TuiPlugin {
    fn name() -> &'static str {
        "tui"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        Ok(Some(serialize_config(Config::default())?))
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config: Config = deserialize_config(config)?;
        Ok(Box::new(TuiPlugin {
            config,
            stop: Arc::new(AtomicBool::new(false)),
            ui_thread: None,
        }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        let stale_after = match self.config.stale_after_seconds {
            0 => None,
            secs => Some(Duration::from_secs(secs)),
        };
        let history_window = Duration::from_secs(self.config.graph_history_seconds.max(1));
        let shared = Arc::new(Mutex::new(Model::new(stale_after, history_window)));

        // The interactive UI needs a real terminal on stdout.
        if std::io::stdout().is_terminal() {
            let shared = shared.clone();
            let stop = self.stop.clone();
            let log_buffer_lines = self.config.log_buffer_lines.max(1) as usize;
            self.ui_thread = Some(
                std::thread::Builder::new()
                    .name("tui".to_owned())
                    .spawn(move || tui::run(shared, stop, log_buffer_lines))?,
            );
        } else {
            log::warn!("tui plugin: stdout is not an interactive terminal, the UI will not be displayed");
        }

        let settings = TuiOutputSettings {
            print_unit: self.config.print_unit,
            use_unit_display_name: self.config.use_unit_display_name,
        };
        alumet.add_blocking_output("out", Box::new(TuiOutput::new(shared, settings)))?;
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        // Signal the UI thread and wait for it to restore the terminal.
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.ui_thread.take() {
            let _ = handle.join();
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Drop a series that has not been updated within this many seconds. Set to `0` to keep every
    /// series forever (not recommended with sources that produce many short-lived series).
    pub stale_after_seconds: u64,
    /// How many seconds of history a graph keeps in memory.
    pub graph_history_seconds: u64,
    /// How many captured log lines (stderr) to keep for the scrollable log table. At a few hundred
    /// bytes per line, the default of 5000 costs on the order of 1-2 MB of RAM.
    pub log_buffer_lines: u64,
    /// Show the metric unit.
    pub print_unit: bool,
    /// Use the unit display name (e.g. `J`) instead of its unique name (e.g. `joule`).
    pub use_unit_display_name: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            stale_after_seconds: 30,
            graph_history_seconds: 120,
            log_buffer_lines: 5000,
            print_unit: true,
            use_unit_display_name: true,
        }
    }
}
