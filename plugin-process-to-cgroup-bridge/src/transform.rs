use alumet::{
    measurement::{MeasurementBuffer, MeasurementPoint, WrappedMeasurementValue},
    metrics::RawMetricId,
    pipeline::{
        elements::{error::TransformError, transform::TransformContext},
        Transform,
    },
    resources::ResourceConsumer,
};
use anyhow::{anyhow, Context};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

// --- TESTS PURPOSE ---
// The following codes functions/variables make it possible to mock PROCFS for tests purpose
// The function `get_proc_path()` will work as following depending on the context:
//  Context Production: Will return a PathBuf with value "/proc"
//
//  Context Tests:      Will return a PathBuf with the value inside the static PROC_PATH variable
//                      This value can be injected by tests using `set_proc_path_for_test` function
//                      Note that this variable can be set only once
#[cfg(test)]
use std::sync::OnceLock;

#[cfg(test)]
static PROC_PATH: OnceLock<PathBuf> = OnceLock::new();

#[cfg(test)]
fn get_proc_path() -> &'static PathBuf {
    PROC_PATH.get_or_init(|| {
        panic!("PROC_PATH not initialized in test - call set_proc_path_for_test() first");
    })
}

#[cfg(test)]
pub fn set_proc_path_for_test(path: PathBuf) {
    let _ = PROC_PATH.set(path);
}

#[cfg(not(test))]
fn get_proc_path() -> PathBuf {
    PathBuf::from("/proc")
}
// --- END OF TESTS PURPOSE ---

pub struct ProcessToCgroupBridgeTransform {
    processes_metrics_ids: Vec<RawMetricId>,
    merge_similar_cgroups: bool,
    keep_processed_measurements: bool,
}

impl Transform for ProcessToCgroupBridgeTransform {
    fn apply(&mut self, measurements: &mut MeasurementBuffer, _ctx: &TransformContext) -> Result<(), TransformError> {
        let mut cgroup_measurements = MeasurementBuffer::new();
        let mut kept_measurements = MeasurementBuffer::new();
        for measurement in measurements.iter() {
            if let Some(cgroup_measurement) = self.cgroup_measurement_from_measurement(measurement)? {
                cgroup_measurements.push(cgroup_measurement);
                if self.keep_processed_measurements {
                    kept_measurements.push(measurement.clone());
                }
            } else {
                kept_measurements.push(measurement.clone());
            }
        }
        measurements.clear();

        if self.merge_similar_cgroups {
            let mut aggregated_cgroup_measurements = Self::aggregate_cgroups_measurements(&mut cgroup_measurements);
            cgroup_measurements.clear();
            cgroup_measurements.merge(&mut aggregated_cgroup_measurements);
        }

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
    ) -> Self {
        Self {
            processes_metrics_ids,
            merge_similar_cgroups,
            keep_processed_measurements,
        }
    }

    fn cgroup_measurement_from_measurement(
        &self,
        measurement: &MeasurementPoint,
    ) -> anyhow::Result<Option<MeasurementPoint>> {
        if !self.processes_metrics_ids.contains(&measurement.metric) {
            return Ok(None);
        }
        let pid = match Self::extract_process_id_from_measurement(measurement) {
            Ok(pid) => pid,
            Err(_) => return Ok(None),
        };
        match Self::find_cgroup_path_from_process_id(pid)? {
            Some(cgroup_path) => {
                let cgroup_consumer = ResourceConsumer::ControlGroup {
                    path: cgroup_path.into(),
                };
                let mut cgroup_measurement = measurement.clone();
                cgroup_measurement.consumer = cgroup_consumer;
                Ok(Some(cgroup_measurement))
            }
            None => {
                log::warn!("no cgroup was found for process {pid}");
                Ok(None)
            }
        }
    }

    fn find_cgroup_path_from_process_id(pid: u32) -> anyhow::Result<Option<String>> {
        let procfs_cgroup_base_path = get_proc_path();

        let procfs_cgroup_filepath = procfs_cgroup_base_path.join(pid.to_string()).join("cgroup");

        if !Path::new(&procfs_cgroup_filepath).exists() {
            return Ok(None);
        }

        let contents = fs::read_to_string(&procfs_cgroup_filepath)
            .with_context(|| format!("Failed to read {:?}", procfs_cgroup_filepath))?;

        // a typical procfs cgroup file will contain only one line
        // eg: 0::/system.slice/docker-7c7fc86f5f2a609c41c6edd65bd1b64135124a687fa6516f6b177b040d6e3b68.scope
        for line in contents.lines() {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() >= 3 {
                let cgroup_path = parts[2];
                if !cgroup_path.is_empty() {
                    return Ok(Some(cgroup_path.to_string()));
                }
            }
        }

        Ok(None)
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
        let mut grouped: HashMap<(RawMetricId, ResourceConsumer, u64), Vec<&MeasurementPoint>> = HashMap::new();

        // Group by (metric_id, consumer, timestamp)
        for point in measurements.iter() {
            let ts = point.timestamp.duration_since(UNIX_EPOCH.into()).unwrap().as_secs();
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
}

fn extract_numeric_value(measurement: &MeasurementPoint) -> Option<f64> {
    match measurement.value {
        WrappedMeasurementValue::F64(v) => Some(v),
        WrappedMeasurementValue::U64(v) => Some(v as f64),
    }
}
