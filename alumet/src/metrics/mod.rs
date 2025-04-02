//! Definition and management of metrics.

pub mod def;
pub mod duplicate;
pub mod error;
pub mod online;
pub mod registry;

pub use def::{Metric, RawMetricId, TypedMetricId};
