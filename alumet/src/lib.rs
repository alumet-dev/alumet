//! ALUMET: Adaptive, Lightweight, Unified Metrics.
//!
//! Alumet is a generic and extensible measurement tool.
//!
//! This crate provides the customizable measurement core.
//! In particular, it offers a measurement pipeline with three steps:
//! 1. Accept measurements from input sources.
//! 2. Transform the measurements.
//! 3. Write the measurements to outputs.
//!
//! The pipeline is backed by asynchronous Tokio tasks.
//! It is designed to be efficient at high frequencies, and to accept any kind of (relatively) low-level measurements.
//!
//! [Sources](pipeline::Source), [transform functions](pipeline::Transform) and [outputs](pipeline::Output)
//! are provided by [plugins](plugin). This crate contains a plugin system to add and remove such elements in a modular architecture.

pub mod agent;
pub mod measurement;
pub mod metrics;
pub mod pipeline;
pub mod plugin;
pub mod resources;
pub mod units;

#[cfg(feature = "dynamic")]
mod ffi;
