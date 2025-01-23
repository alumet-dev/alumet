//! Helpers for creating a measurement agent.
//!
//! # Example
//!
//! ```
//! use alumet::{agent, pipeline};
//! use std::time::Duration;
//!
//! # fn f() -> anyhow::Result<()> {
//! let mut pipeline_builder = pipeline::Builder::new();
//! let mut agent_builder = agent::Builder::new(pipeline_builder);
//! // TODO configure the agent, add the plugins, etc.
//!
//! // start Alumet
//! let agent = agent_builder.build_and_start()?;
//!
//! // initiate shutdown, this can be done from any thread
//! agent.pipeline.control_handle().shutdown();
//!
//! // run until the shutdown command is processed, and stop all the plugins
//! let timeout = Duration::MAX;
//! agent.wait_for_shutdown(timeout)?;
//! # Ok(())
//! # }
//! ```

pub mod builder;
pub mod config;
pub mod exec;
pub mod plugin;

pub use builder::{Builder, RunningAgent};
