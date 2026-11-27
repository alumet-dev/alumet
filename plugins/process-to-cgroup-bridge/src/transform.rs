use alumet::{
    measurement::{MeasurementBuffer, MeasurementPoint, WrappedMeasurementValue},
    metrics::RawMetricId,
    pipeline::{
        Transform,
        elements::{error::TransformError, transform::TransformContext},
    },
    resources::ResourceConsumer,
};
use anyhow::{Context, anyhow};
use std::{
    collections::HashMap,
    fs, io,
    path::PathBuf,
    time::{Duration, Instant, UNIX_EPOCH},
};

pub struct ProcessToCgroupBridgeTransform {
    processes_metrics_ids: Vec<RawMetricId>,
    merge_similar_cgroups: bool,
    keep_processed_measurements: bool,
    fallback_ttl: Duration,
    /// Last cgroup seen for each pid, used when the process is gone by the time its measurements
    /// reach the transform.
    known_cgroups: HashMap<u32, (String, Instant)>,

    #[cfg(test)]
    proc_path: PathBuf,
}

impl Transform for ProcessToCgroupBridgeTransform {
    fn apply(&mut self, measurements: &mut MeasurementBuffer, _ctx: &TransformContext) -> Result<(), TransformError> {
        let ttl = self.fallback_ttl;
        let now = Instant::now();
        self.known_cgroups
            .retain(|_, (_, seen)| now.duration_since(*seen) < ttl);

        let mut cgroup_measurements = MeasurementBuffer::new();
        let mut kept_measurements = MeasurementBuffer::new();
        for measurement in measurements.iter_mut() {
            match self.cgroup_measurement_from_measurement(measurement) {
                Ok(Some(cgroup_measurement)) => {
                    cgroup_measurements.push(cgroup_measurement);
                    if self.keep_processed_measurements {
                        kept_measurements.push(measurement.clone());
                    }
                }
                Ok(None) => kept_measurements.push(measurement.clone()),
                Err(e) => {
                    kept_measurements.push(measurement.clone());
                    log::error!(
                        "error while transforming measurement to cgroup measurement: {e} - keeping old measurement"
                    )
                }
            }
        }

        if self.merge_similar_cgroups {
            let mut aggregated_cgroup_measurements = aggregate_cgroups_measurements(&mut cgroup_measurements);
            cgroup_measurements.clear();
            cgroup_measurements.merge(&mut aggregated_cgroup_measurements);
        }

        measurements.clear();
        measurements.merge(&mut kept_measurements);
        measurements.merge(&mut cgroup_measurements);

        Ok(())
    }
}

impl ProcessToCgroupBridgeTransform {
    pub fn new(
        processes_metrics_ids: Vec<RawMetricId>,
        merge_similar_cgroups: bool,
        keep_processed_measurements: bool,
        fallback_ttl: Duration,

        #[cfg(test)] proc_path: PathBuf,
    ) -> Self {
        Self {
            processes_metrics_ids,
            merge_similar_cgroups,
            keep_processed_measurements,
            fallback_ttl,
            known_cgroups: HashMap::new(),

            #[cfg(test)]
            proc_path,
        }
    }

    fn cgroup_measurement_from_measurement(
        &mut self,
        measurement: &mut MeasurementPoint,
    ) -> anyhow::Result<Option<MeasurementPoint>> {
        if !self.processes_metrics_ids.contains(&measurement.metric) {
            return Ok(None);
        }
        let pid = match extract_process_id_from_measurement(measurement) {
            Ok(pid) => pid,
            Err(_) => return Ok(None),
        };
        let cgroup_path = self.cgroup_path_of(pid)?;

        let cgroup_consumer = ResourceConsumer::ControlGroup {
            path: cgroup_path.into(),
        };
        let mut cgroup_measurement = measurement.clone();
        cgroup_measurement.consumer = cgroup_consumer;
        Ok(Some(cgroup_measurement))
    }

    /// Always prefers procfs, so that a cgroup migration or a pid reuse is never hidden by the cache.
    fn cgroup_path_of(&mut self, pid: u32) -> anyhow::Result<String> {
        let procfs_cgroup_filepath = self.get_proc_path().join(pid.to_string()).join("cgroup");
        match fs::read_to_string(&procfs_cgroup_filepath) {
            Ok(contents) => {
                let path = parse_cgroup_path(pid, &contents)
                    .with_context(|| format!("no cgroup path found in {procfs_cgroup_filepath:?}"))?;
                if !self.fallback_ttl.is_zero() {
                    self.known_cgroups.insert(pid, (path.clone(), Instant::now()));
                }
                Ok(path)
            }
            Err(e) => {
                if e.kind() == io::ErrorKind::NotFound {
                    // The process died after being measured: its last known cgroup is the right one.
                    if let Some((path, seen)) = self.known_cgroups.get(&pid)
                        && seen.elapsed() < self.fallback_ttl
                    {
                        return Ok(path.clone());
                    }
                }
                Err(e).with_context(|| format!("failed to read {procfs_cgroup_filepath:?}"))
            }
        }
    }

    #[cfg(not(test))]
    fn get_proc_path(&self) -> PathBuf {
        PathBuf::from("/proc")
    }

    #[cfg(test)]
    fn get_proc_path(&self) -> PathBuf {
        self.proc_path.clone()
    }
}

fn parse_cgroup_path(pid: u32, contents: &str) -> Option<String> {
    // a procfs cgroupv2 file will contain only one line
    // eg: 0::/system.slice/docker-7c7fc86f5f2a609c41c6edd65bd1b64135124a687fa6516f6b177b040d6e3b68.scope
    //
    // a procfs cgroupv1 file will contains multiple lines
    // eg:
    //     2:memory:/daemons
    //     5:cpuacct,cpu:/daemons
    // in this case we take the first one arbitrary and log a warning
    // todo: implement a matrix between metric names and cgroup v1 controllers to be able to select the right cgroup line
    for line in contents.lines() {
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() >= 3 {
            if !parts[1].is_empty() {
                log::warn!("detected that process {pid} is managed by cgroup v1, selecting the cgroup arbitrarily");
            }
            let cgroup_path = parts[2];
            if !cgroup_path.is_empty() {
                return Some(cgroup_path.to_string());
            }
        }
    }
    None
}

fn extract_process_id_from_measurement(measurement: &MeasurementPoint) -> anyhow::Result<u32> {
    match measurement.consumer {
        ResourceConsumer::Process { pid } => Ok(pid),
        _ => Err(anyhow!(
            "expected a process resource consumer, got something else: {:?}",
            measurement.consumer
        )),
    }
}

/// Aggregates measurements with the same metric, consumer and timestamp by calculating their mean value.
/// Groups are identified by (metric_id, consumer, timestamp) and all measurements in each
/// group are averaged together.
fn aggregate_cgroups_measurements(measurements: &mut MeasurementBuffer) -> MeasurementBuffer {
    let mut grouped: HashMap<(RawMetricId, ResourceConsumer, u128), Vec<&MeasurementPoint>> = HashMap::new();

    // Group by (metric_id, consumer, timestamp)
    for point in measurements.iter() {
        let ts = point.timestamp.duration_since(UNIX_EPOCH.into()).unwrap().as_millis();
        let key = (point.metric, point.consumer.clone(), ts);
        grouped.entry(key).or_default().push(point);
    }

    let mut aggregates = MeasurementBuffer::new();

    // Calculate mean value for every group and create aggregated point
    for ((_metric, _consumer, _timestamp), group) in grouped {
        let (sum, count) = group
            .iter()
            .filter_map(|p| extract_numeric_value(p))
            .fold((0.0, 0), |(sum, count), value| (sum + value, count + 1));

        let mean = if count > 0 { sum / count as f64 } else { 0.0 };

        let mut new_point = group[0].clone(); // cannot panic since group cannot be empty
        new_point.value = WrappedMeasurementValue::F64(mean);
        aggregates.push(new_point);
    }
    aggregates
}

fn extract_numeric_value(measurement: &MeasurementPoint) -> Option<f64> {
    match measurement.value {
        WrappedMeasurementValue::F64(v) => Some(v),
        WrappedMeasurementValue::U64(v) => Some(v as f64),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transform_with_proc(proc_path: PathBuf, fallback_ttl: Duration) -> ProcessToCgroupBridgeTransform {
        ProcessToCgroupBridgeTransform::new(Vec::new(), false, false, fallback_ttl, proc_path)
    }

    fn mock_process(proc_path: &std::path::Path, pid: u32, cgroup: &str) {
        let dir = proc_path.join(pid.to_string());
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("cgroup"), format!("0::{cgroup}\n")).unwrap();
    }

    #[test]
    fn dead_process_falls_back_to_last_known_cgroup() {
        let tmp = tempfile::tempdir().unwrap();
        let mut transform = transform_with_proc(tmp.path().to_path_buf(), Duration::from_secs(60));
        mock_process(tmp.path(), 1, "/a.slice");

        assert_eq!(transform.cgroup_path_of(1).unwrap(), "/a.slice");
        fs::remove_dir_all(tmp.path().join("1")).unwrap();
        assert_eq!(transform.cgroup_path_of(1).unwrap(), "/a.slice");
    }

    #[test]
    fn running_process_ignores_cache() {
        let tmp = tempfile::tempdir().unwrap();
        let mut transform = transform_with_proc(tmp.path().to_path_buf(), Duration::from_secs(60));
        mock_process(tmp.path(), 1, "/a.slice");
        transform.cgroup_path_of(1).unwrap();

        mock_process(tmp.path(), 1, "/b.slice");
        assert_eq!(transform.cgroup_path_of(1).unwrap(), "/b.slice");
    }

    #[test]
    fn expired_entry_is_not_used() {
        let tmp = tempfile::tempdir().unwrap();
        let mut transform = transform_with_proc(tmp.path().to_path_buf(), Duration::ZERO);
        mock_process(tmp.path(), 1, "/a.slice");
        transform.cgroup_path_of(1).unwrap();

        fs::remove_dir_all(tmp.path().join("1")).unwrap();
        assert!(transform.cgroup_path_of(1).is_err());
    }

    #[test]
    fn unknown_dead_process_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let mut transform = transform_with_proc(tmp.path().to_path_buf(), Duration::from_secs(60));
        assert!(transform.cgroup_path_of(42).is_err());
    }
}
