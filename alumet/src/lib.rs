//! ALUMET: Adaptive, Lightweight, Unified Metrics.
//!
//! Alumet is a tool that allows you to measure things: cpu usage, energy consumption, etc.
//!
//! Unlike other measurement tools, Alumet is a **modular framework**.
//! The `alumet` crate enables you to create a bespoke tool with specific
//! probes (for example RAPL energy counters or perf profiling data),
//! transformation functions (such as mathematical models), and outputs (such as a timeseries database).
//!
//! # This crate
//! This crate provides the customizable measurement core.
//!
//! In particular, it offers a measurement pipeline with three steps:
//! 1. Accept measurements from input [sources](pipeline::Source).
//! 2. [Transform](pipeline::Transform) the measurements.
//! 3. Write the measurements to [outputs](pipeline::Output).
//!
//! The pipeline is backed by asynchronous **Tokio** tasks.
//! It is designed to be generic, reconfigurable and efficient.
//! It can support a high number of sources and run them at high frequencies (e.g. above 1000 Hz).
//! Benchmarks will be provided in the future.
//!
//! # Agents and plugins
//! Customization is made possible thanks to a plugin system.
//! The Alumet core does not measure anything by itself.
//! Instead, plugins provide the [Sources](pipeline::Source), [transform functions](pipeline::Transform) and [outputs](pipeline::Output) of the measurement pipeline.
//!
//! To do something useful with Alumet, you need:
//! - A runnable application, the _agent_. See the documentation of the [`agent`] module.
//! - A set of _plugins_, which implement the measurement and export operations. Learn how to make plugins by reading the documentation of the [`plugin`] module.

#![doc(html_logo_url = "https://alumet.dev/img/alumet-logo-color.svg")]

pub mod agent;
pub mod measurement;
pub mod metrics;
pub mod pipeline;
pub mod plugin;
pub mod resources;
pub mod units;

pub const VERSION: &'static str = env!("CARGO_PKG_VERSION");
