//! ALUMET: Adaptive, Lightweight, Unified Metrics.
//!
//! Alumet is a tool that allows you to measure things: cpu usage, energy consumption, etc.
//!
//! Unlike other measurement tools, Alumet is a **modular framework**.
//! The `alumet` crate enables you to create a bespoke tool with specific probes
//! (for example RAPL energy counters or perf profiling data) and outputs (such as a timeseries database).
//!
//! # This crate
//! This crate provides the customizable measurement core.
//!
//! In particular, it offers a measurement pipeline with three steps:
//! 1. Accept measurements from input [sources](pipeline::Source).
//! 2. [Transform](pipeline::Transform) the measurements.
//! 3. Write the measurements to [outputs](pipeline::Output).
//!
//! Customization is made possible thanks to a plugin system.
//! The Alumet core does not measure anything by itself.
//! Instead, plugins provide the [Sources](pipeline::Source), [transform functions](pipeline::Transform) and [outputs](pipeline::Output) of the measurement pipeline.
//!
//! The pipeline is backed by asynchronous **Tokio** tasks.
//! It is designed to be generic, efficient, reconfigurable and to support high frequencies (~ 1000 Hz).
//! Benchmarks will be provided soon.
//!
//! # Agents and plugins
//! To do something useful with Alumet, you need:
//! - A runnable application, the _agent_. See the documentation of the [`agent`] module.
//! - A set of _plugins_, which implement the measurement and export operations. Learn how to make plugins by reading the documentation of the [`plugin`] module.
//!

pub mod agent;
pub mod measurement;
pub mod metrics;
pub mod pipeline;
pub mod plugin;
pub mod resources;
pub mod units;

#[cfg(feature = "dynamic")]
mod ffi;
