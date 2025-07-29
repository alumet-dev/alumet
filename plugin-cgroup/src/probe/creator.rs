use std::{ops::ControlFlow, time::Duration};

use alumet::pipeline::{
    control::{request, PluginControlHandle},
    Source,
};
use anyhow::Context;
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
        alumet_control: PluginControlHandle,
    ) -> anyhow::Result<CgroupProbeCreator> {
        let callback = WaitCallback::new(metrics, personaliser, alumet_control);
        let _wait = CgroupMountWait::new(config.v1_coalesce_delay, callback)?;
        Ok(Self { _wait })
    }
}

/// A callback with a modifiable state.
struct WaitCallback<P: ProbePersonaliser> {
    detectors: Vec<CgroupDetector>,
    rt: tokio::runtime::Runtime,
    state: CloneableState<P>,
}

/// The state of the callback closure.
/// In a sub-structure to make it easier to clone when moving into a closure.
#[derive(Clone)]
struct CloneableState<P: ProbePersonaliser> {
    // Store the detectors so that they keep working.
    metrics: Metrics,
    personaliser: P,
    alumet_control: PluginControlHandle,
}

impl<P: ProbePersonaliser> WaitCallback<P> {
    fn new(metrics: Metrics, personaliser: P, alumet_control: PluginControlHandle) -> Self {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("I need a current-thread runtime");
        Self {
            detectors: Vec::new(),
            state: CloneableState {
                metrics,
                personaliser,
                alumet_control,
            },
            rt,
        }
    }
}

impl<P: ProbePersonaliser> mount_wait::Callback for WaitCallback<P> {
    fn on_cgroupfs_mounted(&mut self, hierarchies: Vec<CgroupHierarchy>) -> anyhow::Result<ControlFlow<()>> {
        const DISPATCH_TIMEOUT: Duration = Duration::from_secs(1);

        for h in hierarchies {
            let config = detect::Config::default();
            let rt = self.rt.handle().clone();
            let mut state = self.state.clone();
            let detector = CgroupDetector::new(
                h,
                config,
                detect::callback(move |cgroups| {
                    // create the sources
                    let mut sources = Vec::with_capacity(cgroups.len());
                    for cgroup in cgroups {
                        // personalise the source
                        let personalised = state.personaliser.personalise(&cgroup, &state.metrics);
                        // create the source
                        match make_cgroup_source(cgroup, personalised.metrics) {
                            Ok(source) => {
                                sources.push((source, personalised.source_settings));
                            }
                            Err(e) => {
                                // don't fail if only one source fails to be created, try the other ones
                                log::error!("cgroup source creation failed: {e:?}")
                            }
                        }
                    }

                    // spawn the sources on the Alumet pipeline
                    for (source, pers) in sources {
                        // TODO spawn the source in a Paused state if requested by the personaliser
                        let dispatch_task = state.alumet_control.dispatch(
                            request::create_one().add_source(&pers.name, source, pers.trigger),
                            DISPATCH_TIMEOUT,
                        );
                        rt.block_on(dispatch_task)
                            .context("dispatch of source creation request failed")?;
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
