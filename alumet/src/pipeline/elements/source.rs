//! Implementation and control of source tasks.

pub mod builder;
pub mod control;
pub mod error;
pub mod interface;
pub mod run;
mod task_controller;

pub use error::PollError;
pub use interface::{AutonomousSource, Source};
