use std::sync::{Arc, Mutex};

use alumet::{
    measurement::{AttributeValue, MeasurementBuffer},
    pipeline::{
        Transform,
        elements::{error::TransformError, transform::TransformContext},
    },
    resources::ResourceConsumer,
};
use anyhow::anyhow;
use util_cgroups::{Cgroup, CgroupHierarchy, CgroupVersion};
use crate::cgroup_events::CgroupFsMountCallback;


pub trait JobTagger : Send {
    fn attributes_for_cgroup(&self, cgroup: &Cgroup) -> Vec<(String, AttributeValue)>;
}


/// Adds job-related attributes to cgroup measurements that do not have these attributes yet.
pub struct JobAnnotationTransform<T: JobTagger> {
    pub tagger: T,
    pub cgroup_v2_hierarchy: CachedCgroupHierarchy,
}

impl<T: JobTagger> Transform  for JobAnnotationTransform<T> {
    fn apply(&mut self, measurements: &mut MeasurementBuffer, _: &TransformContext) -> Result<(), TransformError> {
        for m in measurements.iter_mut() {
            if let ResourceConsumer::ControlGroup { path: cgroup_path } = &m.consumer
                && !m.attributes_keys().any(|key| key == "job_id")
            {
                // This is a cgroup measurement that does not have a job_id, try to map it to a job.
                //
                // The main problem is: how to get the hierarchy here?
                // If we are in cgroup v2, we could find the unique hierarchy on startup and use it here.
                // But on v1, we would need to modify the consumer's struct… => v1 NOT SUPPORTED at the moment

                let Some(hierarchy) = self.cgroup_v2_hierarchy.fetch() else {
                    return Err(TransformError::UnexpectedInput(anyhow!(
                        "cgroup measurements found but no cgroup v2 hierarchy has been detected. Are you using cgroup v2? (v1 not supported by this plugin)"
                    )));
                };

                let cgroup = Cgroup::from_cgroup_path(hierarchy, cgroup_path.to_string());
                let sysfs_path = cgroup.fs_path();
                match sysfs_path.try_exists() {
                    Ok(true) => {
                        let job_attrs = self.tagger.attributes_for_cgroup(&cgroup);
                        for (k, v) in job_attrs {
                            m.add_attr(k, v);
                        }
                    }
                    Ok(false) => {
                        log::warn!(
                            "cgroup does not exist: {sysfs_path:?}. Are you using cgroup v2? (or did the cgroup disappear quickly?)"
                        );
                    }
                    Err(err) => {
                        log::warn!("failed to check the existence of cgroup {sysfs_path:?}: {err}")
                    }
                }
            }
        }
        Ok(())
    }
    
    fn finish(&mut self, ctx: &TransformContext) -> Result<(), TransformError> {
        Ok(())
    }
}

/// Thread-safe shared value that stores the cgroup v2 hierarchy (for use by the transform).
#[derive(Clone, Default)]
pub struct SharedCgroupHierarchy(Arc<Mutex<Option<CgroupHierarchy>>>);

impl SharedCgroupHierarchy {
    fn set(&self, value: CgroupHierarchy) {
        *self.0.lock().unwrap() = Some(value);
    }
}

/// A wrapper around `Option<SharedCgroupHierarchy>` that implements [`CgroupFsMountCallback`].
#[derive(Clone, Default)]
pub struct OptionalSharedHierarchy(Option<SharedCgroupHierarchy>);

impl OptionalSharedHierarchy {
    pub fn enable(&mut self, shared: SharedCgroupHierarchy) {
        self.0 = Some(shared);
    }
}

impl CgroupFsMountCallback for OptionalSharedHierarchy {
    fn on_cgroupfs_mounted(&mut self, cgroupfs: &Vec<CgroupHierarchy>) -> anyhow::Result<()> {
        if let Some(shared) = &mut self.0 {
            // Find the cgroup v2 hierarchy (there is at most one) and save it, so that the transform can use it.
            for h in cgroupfs {
                if h.version() == CgroupVersion::V2 {
                    log::debug!("found cgroup v2 hierarchy: {:?} - setting shared state", h.root());
                    shared.set(h.clone());
                }
            }
        }
        Ok(())
    }
}

/// Cached version of `SharedCgroupHierarchy` that improves read latency.
pub struct CachedCgroupHierarchy {
    /// Local value for faster access.
    cached: Option<CgroupHierarchy>,
    /// Value filled by another thread.
    shared: SharedCgroupHierarchy,
}

impl CachedCgroupHierarchy {
    pub fn new(shared: SharedCgroupHierarchy) -> Self {
        Self { cached: None, shared }
    }

    /// Returns a reference to the hierarchy, if available.
    ///
    /// ## Caching
    /// The first time this method is called, it will read the `SharedCgroupHierarchy`.
    /// If the shared value has been set (typically by a background task on another thread),
    /// the cache is updated and returned.
    /// If the shared value has not been set, this method returns `None`.
    ///
    /// Once this method returns `Some`, it will not look at the shared value anymore,
    /// but will always return the cached value.
    pub fn fetch(&mut self) -> &Option<CgroupHierarchy> {
        match &mut self.cached {
            Some(_) => (),
            opt @ None => {
                log::trace!("fetching the shared cgroup hierarchy");
                *opt = self.shared.0.lock().unwrap().take();
                log::trace!("got: {opt:?}");
            }
        };
        &self.cached
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn test_shared_value() {
        let shared = SharedCgroupHierarchy::default();
        let shared_a = shared.clone();
        let mut cached_b = CachedCgroupHierarchy::new(shared.clone());

        assert!(
            cached_b.fetch().is_none(),
            "fetch() should return None because the shared value has not been set yet"
        );

        let thread_a = std::thread::spawn(move || {
            shared_a.set(CgroupHierarchy::manually_unchecked(
                "/sys/fs/test",
                CgroupVersion::V2,
                vec!["cpuset"],
            ));
        });

        let thread_b = std::thread::spawn(move || {
            let mut i = 0;
            loop {
                if let Some(h) = cached_b.fetch() {
                    assert_eq!(h.version(), CgroupVersion::V2, "unexpected hierarchy");
                    break;
                }
                std::thread::sleep(Duration::from_millis(1));
                i += 1;
                if i > 500 {
                    panic!("it should not take that long to write and read a shared value")
                }
            }
            assert!(
                cached_b.shared.0.lock().unwrap().is_none(),
                "the shared value should have been taken by fetch"
            );
            assert!(
                cached_b.fetch().is_some(),
                "after the first successfull fetch, it should always return Some"
            );
        });

        thread_a.join().unwrap();
        thread_b.join().unwrap();
    }
}
