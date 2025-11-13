/// React to cgroupfs changes, v1 and v2.
pub mod cgroup_events;
/// Probe for cgroups v1.
pub mod v1;
/// Probe for cgroups v2.
pub mod v2;

pub mod delta;
pub mod job_annotation_transform;
pub mod metrics;
pub mod regex;
mod self_stop;
