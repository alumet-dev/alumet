use std::{
    ops::ControlFlow,
    sync::{Arc, Mutex},
    time::Duration,
};

use alumet::pipeline::{
    Source,
    control::{PluginControlHandle, request},
    elements::source::trigger::TriggerSpec,
};
use anyhow::Context;
use util_cgroups::{Cgroup, CgroupDetector, CgroupHierarchy, CgroupMountWait, CgroupVersion, detect, mount_wait};

use crate::{
    metrics::{AugmentedMetrics, Metrics},
    v1::CgroupV1Probe,
    v2::CgroupV2Probe,
};

/// Reacts to the deletion of cgroups.
pub trait CgroupRemovalCallback: Clone + Send + 'static {
    fn on_cgroups_removed(&mut self, cgroups: Vec<Cgroup>) -> anyhow::Result<()>;
}

/// Prepares a cgroup "probe" (which is an Alumet source).
pub trait CgroupSetupCallback: Clone + Send + 'static {
    /// Prepares a new probe. Returns `None` to skip the creation of the probe.
    fn setup_new_probe(&mut self, cgroup: &Cgroup, metrics: &Metrics) -> Option<ProbeSetup>;
}

/// A [`CgroupRemovalCallback`] that does nothing.
#[derive(Clone, Copy)]
pub struct NoCallback;

impl CgroupRemovalCallback for NoCallback {
    fn on_cgroups_removed(&mut self, _cgroups: Vec<Cgroup>) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Settings to apply to the cgroup probe.
pub struct ProbeSetup {
    pub metrics: AugmentedMetrics,
    pub source_settings: SourceSettings,
}

#[derive(Debug, Clone)]
pub struct SourceSettings {
    pub name: String,
    pub trigger: TriggerSpec,
}

/// Reacts to cgroup events.
#[allow(dead_code)] // fields only exist to keep some values alive
pub struct CgroupReactor {
    wait: CgroupMountWait,
    detectors: AliveDetectors,
}

/// Configuration of the [`CgroupReactor`].
#[derive(Debug)]
pub struct ReactorConfig {
    /// If None, every cgroupfs v1 immediately triggers the callback.
    ///
    /// If Some, the detection events of cgroupfs v1 are coalesced together if they are close enough. Since multiple cgroupfs v1 are often mounted together, it is generally a good idea to use this parameter. The **default value** is 1 second.
    ///
    /// When the first cgroupfs v1 is detected, a timer starts. It is stopped after the given delay. Every cgroupfs v1 detected before the timer stops is pushed to the same list as the first cgroupfs v1. When the timer stops, the callback is triggered with the list, only once for all the detected cgroupfs v1.
    pub v1_coalesce_delay: Option<Duration>,

    /// Interval between two scans of the cgroup v1 hierarchies.
    ///
    /// The cgroup v1 filesystem does not support notifications (inotify), we must rely on manually inspecting the cgroups at regular intervals.
    /// If `None` (the default), uses the default value of [`detect::Config`].
    pub v1_refresh_interval: Option<Duration>,
}

impl Default for ReactorConfig {
    fn default() -> Self {
        Self {
            v1_coalesce_delay: Some(Duration::from_secs(1)),
            v1_refresh_interval: None,
        }
    }
}

#[derive(Clone)]
pub struct ReactorCallbacks<S: CgroupSetupCallback, R: CgroupRemovalCallback> {
    /// Called when a new cgroup is detected.
    ///
    /// Its role is to setup the probe (source) associated to this cgroup.
    /// It can also prevent the creation of the probe.
    pub probe_setup: S,

    /// Called when a cgroup is deleted.
    pub on_removal: R,
}

/// Keeps the CgroupDetectors alive until the CgroupReactor goes away.
///
/// Holding the `CgroupMountWait` is not enough, because the background thread can stop after an event
/// (when the wait callback returns `ControlFlow::Break`), which will drop its state, terminating the
/// detectors if they are owned by the thread.
#[derive(Clone)]
struct AliveDetectors {
    inner: Arc<Mutex<Vec<CgroupDetector>>>,
}

impl AliveDetectors {
    fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn push(&mut self, detector: CgroupDetector) {
        self.inner.lock().unwrap().push(detector);
    }
}

struct WaitCallback<S: CgroupSetupCallback, R: CgroupRemovalCallback> {
    detectors: AliveDetectors,
    rt: tokio::runtime::Runtime,
    state: CloneableState<S, R>,
}

struct DetectionCallback<S: CgroupSetupCallback, R: CgroupRemovalCallback> {
    rt: tokio::runtime::Handle,
    state: CloneableState<S, R>,
}

/// The state of the callback closure.
/// In a sub-structure to make it easier to clone when moving into a closure.
#[derive(Clone)]
struct CloneableState<S: CgroupSetupCallback, R: CgroupRemovalCallback> {
    // Store the detectors so that they keep working.
    metrics: Metrics,
    callbacks: ReactorCallbacks<S, R>,
    alumet_control: PluginControlHandle,
    detector_config: detect::Config,
}

impl CgroupReactor {
    /// Configures and starts a cgroup "notification" system with some callbacks that will:
    /// - automatically the mounting of cgroupfs
    /// - create an Alumet source for every new cgroup (some cgroups can be skipped, depending on the setup callback)
    /// - react to the removal of cgroups
    pub fn new(
        config: ReactorConfig,
        metrics: Metrics,
        callbacks: ReactorCallbacks<impl CgroupSetupCallback, impl CgroupRemovalCallback>,
        alumet_control: PluginControlHandle,
    ) -> anyhow::Result<Self> {
        let detectors = AliveDetectors::new();
        let mut detector_config = detect::Config::default();
        if let Some(refresh_interval) = config.v1_refresh_interval {
            detector_config.v1_refresh_interval = refresh_interval;
        }
        let callback = WaitCallback::new(metrics, callbacks, alumet_control, detectors.clone(), detector_config);
        let wait = CgroupMountWait::new(config.v1_coalesce_delay, callback)?;
        Ok(Self { wait, detectors })
    }
}

impl<S: CgroupSetupCallback, R: CgroupRemovalCallback> WaitCallback<S, R> {
    fn new(
        metrics: Metrics,
        callbacks: ReactorCallbacks<S, R>,
        alumet_control: PluginControlHandle,
        detectors: AliveDetectors,
        detector_config: detect::Config,
    ) -> Self {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("I need a current-thread runtime");
        Self {
            detectors,
            state: CloneableState {
                metrics,
                callbacks,
                alumet_control,
                detector_config,
            },
            rt,
        }
    }
}

impl<S: CgroupSetupCallback, R: CgroupRemovalCallback> mount_wait::Callback for WaitCallback<S, R> {
    fn on_cgroupfs_mounted(&mut self, hierarchies: Vec<CgroupHierarchy>) -> anyhow::Result<ControlFlow<()>> {
        for h in hierarchies {
            let config = self.state.detector_config.clone();
            let callback = DetectionCallback {
                rt: self.rt.handle().clone(),
                state: self.state.clone(),
            };
            let detector = CgroupDetector::new(h, config, callback)?;
            self.detectors.push(detector);
        }
        Ok(ControlFlow::Break(()))
    }
}

const DISPATCH_TIMEOUT: Duration = Duration::from_secs(1);

impl<S: CgroupSetupCallback, R: CgroupRemovalCallback> detect::Callback for DetectionCallback<S, R> {
    fn on_cgroups_created(&mut self, cgroups: Vec<Cgroup>) -> anyhow::Result<()> {
        log::debug!("detected new cgroups: {cgroups:?}");

        // create the sources
        let mut sources = Vec::with_capacity(cgroups.len());
        for cgroup in cgroups {
            // setup the source
            let setup = self
                .state
                .callbacks
                .probe_setup
                .setup_new_probe(&cgroup, &self.state.metrics);
            match setup {
                Some(s) => {
                    // create the source
                    log::debug!("creating a source for cgroup {}", cgroup.unique_name());
                    match make_cgroup_source(cgroup, s.metrics) {
                        Ok(source) => {
                            sources.push((source, s.source_settings));
                        }
                        Err(e) => {
                            // don't fail if only one source fails to be created, try the other ones
                            log::error!("cgroup source creation failed: {e:?}")
                        }
                    }
                }
                None => {
                    // don't create the source
                    log::debug!("no source will be created for cgroup {}", cgroup.unique_name());
                }
            }
        }

        // spawn the sources on the Alumet pipeline
        for (source, pers) in sources {
            // TODO spawn the source in a Paused state if requested by the setup
            let dispatch_task = self.state.alumet_control.dispatch(
                request::create_one().add_source(&pers.name, source, pers.trigger),
                DISPATCH_TIMEOUT,
            );
            self.rt
                .block_on(dispatch_task)
                .context("dispatch of source creation request failed")?;
        }
        Ok(())
    }

    fn on_cgroups_removed(&mut self, cgroups: Vec<Cgroup>) -> anyhow::Result<()> {
        // The source will stop itself: it will try to gather measurements and see that the cgroup no longer exists.
        // What we do here is delegate the work to someone else, because it depends on the context.
        // Some plugins may want to keep track of the active cgroups, others may want to send a notification, etc.
        self.state
            .callbacks
            .on_removal
            .on_cgroups_removed(cgroups)
            .context("error in cgroup removal callback")
    }
}

fn make_cgroup_source(cgroup: Cgroup<'_>, metrics: AugmentedMetrics) -> anyhow::Result<Box<dyn Source>> {
    match cgroup.hierarchy().version() {
        CgroupVersion::V1 => Ok(Box::new(CgroupV1Probe::new(cgroup, metrics)?)),
        CgroupVersion::V2 => Ok(Box::new(CgroupV2Probe::new(cgroup, metrics)?)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_remove_callback() {
        // ensure that it compiles
        fn _f(probe_setup: impl CgroupSetupCallback) {
            ReactorCallbacks {
                probe_setup,
                on_removal: NoCallback,
            };
        }
    }
}
