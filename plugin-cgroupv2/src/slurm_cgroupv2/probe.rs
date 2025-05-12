use std::path::{Path, PathBuf};

use alumet::{measurement::{AttributeValue, MeasurementAccumulator, MeasurementPoint, Timestamp}, pipeline::{elements::error::PollError, Source}};
use anyhow::{Context, Result};
use walkdir::WalkDir;

// use super::utils::{gather_value, CgroupV2MetricFile};
use crate::cgroupv2::{Cgroupv2Probe, Metrics};

pub struct SlurmV2prob {
    cgroupv2: Cgroupv2Probe,
    name: String,
}

impl SlurmV2prob {
    pub fn collect_measurements(&mut self, timestamp: Timestamp) -> Result<Vec<MeasurementPoint>, PollError> {
        self.cgroupv2.collect_measurements(timestamp)
    }
    pub fn source_name(&self) -> String {
        format!("job:{}", self.name)
    }
}

impl Source for SlurmV2prob {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        for point in self.collect_measurements(timestamp)? {
            measurements.push(point);
        }
        Ok(())
    }
}

pub fn get_all_job_probes(root_directory_path: &Path, metrics: Metrics) -> Result<Vec<SlurmV2prob>> {
    let mut probes: Vec<SlurmV2prob> = Vec::new();

    for entry in WalkDir::new(root_directory_path)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_dir() && e.depth() <= 1)
    {
        let path = entry.path();
        let job_name = path
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("No file name found"))?
            .to_str()
            .context("Filename is not valid UTF-8")?
            .to_string();
        probes.push(get_job_probe(path.to_path_buf(), metrics.clone(), job_name)?);
    }
    Ok(probes)
}


pub fn get_job_probe(path: PathBuf, metrics: Metrics, job_name: String) -> Result<SlurmV2prob> {
    let base_attrs = vec![("job_name".to_string(), AttributeValue::String(job_name.clone()))];
    let mut cgroup_probe = Cgroupv2Probe::new_from_cgroup_dir(path, metrics)?;
    cgroup_probe.add_additional_attrs(base_attrs);
    // Ok(OAR3JobProbe::new(cgroup_probe, job_name)?)
    Ok(SlurmV2prob { cgroupv2: cgroup_probe, name: job_name })
}
