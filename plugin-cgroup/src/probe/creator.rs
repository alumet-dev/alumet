use std::{ops::ControlFlow, time::Duration};

use alumet::pipeline::Source;
use util_cgroups::{detect, mount_wait, Cgroup, CgroupDetector, CgroupHierarchy, CgroupMountWait, CgroupVersion};

use super::{personalise::ProbePersonaliser, v1::CgroupV1Probe, v2::CgroupV2Probe};
use crate::probe::{AugmentedMetrics, Metrics};

/// Automatically creates new sources when cgroups are created on the system.
pub struct CgroupProbeCreator {
    _wait: CgroupMountWait,
}

/// Configuration of the creator.
#[derive(Debug)]
pub struct Config {
    /// If None, every cgroupfs v1 immediately triggers the callback.
    ///
    /// If Some, the detection events of cgroupfs v1 are coalesced together if they are close enough. Since multiple cgroupfs v1 are often mounted together, it is generally a good idea to use this parameter. The default value is 1 second.
    ///
    /// When the first cgroupfs v1 is detected, a timer starts. It is stopped after the given delay. Every cgroupfs v1 detected before the timer stops is pushed to the same list as the first cgroupfs v1. When the timer stops, the callback is triggered with the list, only once for all the detected cgroupfs v1.
    pub v1_coalesce_delay: Option<Duration>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            v1_coalesce_delay: Some(Duration::from_secs(1)),
        }
    }
}

impl CgroupProbeCreator {
    /// Configures and starts a cgroup "notification" system with a callback that will create an Alumet source for every cgroup.
    pub fn new(
        config: Config,
        metrics: Metrics,
        personaliser: impl ProbePersonaliser,
    ) -> anyhow::Result<CgroupProbeCreator> {
        let callback = WaitCallback::new(metrics, personaliser);
        let _wait = CgroupMountWait::new(config.v1_coalesce_delay, callback)?;
        Ok(Self { _wait })
    }
}

/// A callback with a modifiable state.
struct WaitCallback<P: ProbePersonaliser> {
    // Store the detectors so that they keep working.
    detectors: Vec<CgroupDetector>,
    metrics: Metrics,
    personaliser: P,
}

impl<P: ProbePersonaliser> WaitCallback<P> {
    fn new(metrics: Metrics, personaliser: P) -> Self {
        Self {
            detectors: Vec::new(),
            metrics,
            personaliser,
        }
    }
}

impl<P: ProbePersonaliser> mount_wait::Callback for WaitCallback<P> {
    fn on_cgroupfs_mounted(&mut self, hierarchies: Vec<CgroupHierarchy>) -> anyhow::Result<ControlFlow<()>> {
        for h in hierarchies {
            let metrics = self.metrics.clone();
            let config = detect::Config::default();
            let mut p = self.personaliser.clone();
            let detector = CgroupDetector::new(
                h,
                config,
                detect::callback(move |cgroups| {
                    for cgroup in cgroups {
                        let augmented_metrics = p.personalise(&cgroup, &metrics);
                        if let Err(e) = make_cgroup_source(cgroup, augmented_metrics) {
                            log::error!("cgroup source creation failed: {e:?}");
                        }
                        // TODO spawn the source!
                    }
                    Ok(())
                }),
            )?;
            self.detectors.push(detector);
        }
        Ok(ControlFlow::Break(()))
    }
}

fn make_cgroup_source(cgroup: Cgroup<'_>, metrics: AugmentedMetrics) -> anyhow::Result<Box<dyn Source>> {
    match cgroup.hierarchy().version() {
        CgroupVersion::V1 => Ok(Box::new(CgroupV1Probe::new(cgroup, metrics)?)),
        CgroupVersion::V2 => Ok(Box::new(CgroupV2Probe::new(cgroup, metrics)?)),
    }
}
