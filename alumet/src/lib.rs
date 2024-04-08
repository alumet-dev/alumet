//! ALUMET: Adaptive, Lightweight, Unified Metrics.
//! 
//! Alumet is a generic, extensible and low-level measurement tool.
//! 
//! This crate provides a measurement pipeline with three steps:
//! 1. Accept measurements from input sources.
//! 2. Transform the measurements.
//! 3. Write the measurements to outputs.
//! 
//! The pipeline is backed by asynchronous Tokio tasks.
//! Inputs, transform functions and outputs are provided by plugins.

pub mod config;
pub mod metrics;
pub mod pipeline;
pub mod plugin;
pub mod units;
pub mod resources;
pub mod util;

#[cfg(feature = "dynamic")]
pub mod ffi;
