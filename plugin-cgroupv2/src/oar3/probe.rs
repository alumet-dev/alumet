use alumet::{
    measurement::{AttributeValue, MeasurementAccumulator, Timestamp},
    pipeline::{elements::error::PollError, Source},
};
use anyhow::{Context, Result};

use crate::cgroupv2::{Cgroupv2Probe, Metrics};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub struct OAR3JobProbe {
    cgroupv2: Cgroupv2Probe,
    name: String,
}

impl OAR3JobProbe {
    pub fn new(cgroupv2: Cgroupv2Probe, name: String) -> Result<Self, anyhow::Error> {
        Ok(Self { cgroupv2, name })
    }

    pub fn source_name(&self) -> String {
        format!("job:{}", self.name)
    }
}

impl Source for OAR3JobProbe {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        self.cgroupv2.collect_measurements(timestamp, measurements)?;
        Ok(())
    }
}

pub fn get_all_job_probes(root_directory_path: &Path, metrics: Metrics) -> Result<Vec<OAR3JobProbe>> {
    let mut probes: Vec<OAR3JobProbe> = Vec::new();

    for entry in WalkDir::new(root_directory_path)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_dir())
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

pub fn get_job_probe(path: PathBuf, metrics: Metrics, job_name: String) -> Result<OAR3JobProbe> {
    let base_attrs = vec![("job_name".to_string(), AttributeValue::String(job_name.clone()))];
    let mut cgroup_probe = Cgroupv2Probe::new_from_cgroup_dir(path, metrics)?;
    cgroup_probe.add_additional_attrs(base_attrs);
    OAR3JobProbe::new(cgroup_probe, job_name)
}
