//! Helpers for creating a runnable application based on Alumet, aka an "Alumet agent".
//!
//! # Minimal Example
//!
//! Building an Alumet agent require two key components:
//! - a measurement [pipeline](crate::pipeline)
//! - and a [set of plugins](PluginSet).
//!
//! Use the [`Builder`] to combine them and apply other settings.
//!
//! ```no_run
//! use alumet::{agent, pipeline, static_plugins};
//! use alumet::plugin::rust::AlumetPlugin;
//!
//! use std::time::Duration;
//!
//! # use anyhow;
//! # use alumet::plugin::{AlumetPluginStart, ConfigTable};
//! #
//! struct PluginA;
//! impl AlumetPlugin for PluginA {
//! #    // Plugin implementation
//! #    fn name() -> &'static str {
//! #        "a"
//! #    }
//! #     
//! #    fn version() -> &'static str {
//! #        "0.1.0"
//! #    }
//! #    
//! #    fn init(_config: ConfigTable) -> anyhow::Result<Box<Self>> {
//! #        todo!()
//! #    }
//! #    
//! #    fn start(&mut self, _alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
//! #        todo!()
//! #    }
//! #    
//! #    fn stop(&mut self) -> anyhow::Result<()> {
//! #        todo!()
//! #    }
//! #     
//! #    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
//! #        Ok(None)
//! #    }
//! }
//!
//! // Load the plugin metadata
//! let mut plugins = agent::plugin::PluginSet::new(static_plugins![PluginA]);
//!
//! // Set up the measurement pipeline
//! let mut pipeline = pipeline::Builder::new();
//! pipeline.normal_threads(2); // Example setting: use 2 threads to run async pipeline elements
//!
//! // Build and start the agent
//! let agent = agent::Builder::from_pipeline(plugins, pipeline)
//!     .build_and_start()
//!     .expect("startup failure");
//!
//! // Run until shutdown (you can use Ctrl+C to initiate shutdown from the terminal)
//! agent.wait_for_shutdown(Duration::MAX).expect("error while running");
//! ```
//!
//! # Configuration Management
//!
//! Use the [`config`] module to manage a TOML configuration file that contains both
//! the general agent options and the configuration of each plugin.

pub mod builder;
pub mod config;
pub mod exec;
pub mod plugin;

pub use builder::{Builder, RunningAgent};
