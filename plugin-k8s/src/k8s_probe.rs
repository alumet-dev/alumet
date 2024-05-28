use alumet::{
    measurement::{AttributeValue, MeasurementAccumulator, MeasurementPoint, Timestamp},
    metrics::{MetricCreationError, TypedMetricId},
    plugin::{
        util::{CounterDiff, CounterDiffUpdate},
        AlumetStart,
    },
    resources::{Resource, ResourceConsumer},
    units::{PrefixedUnit, Unit},
};
use anyhow::Result;
use inotify::EventMask;
use std::{fs::File, path::PathBuf};

use crate::cgroup_v2::{self, CgroupV2MetricFile};

use super::INOTIFY_VAR;
use super::MAP_FD;

pub(crate) const CGROUP_MAX_TIME_COUNTER: u64 = u64::MAX;

/// Energy probe based on perf_event for intel RAPL.
pub struct K8SProbe {
    pub metrics: Metrics,
    pub metric_and_counter: Vec<(CgroupV2MetricFile, CounterDiff, CounterDiff, CounterDiff)>,
}

#[derive(Clone)]
pub struct Metrics {
    pub time_used_tot: TypedMetricId<u64>,
    pub time_used_user_mode: TypedMetricId<u64>,
    pub time_used_system_mode: TypedMetricId<u64>,
}

impl K8SProbe {
    pub fn new(metric: Metrics, final_li_metric: Vec<CgroupV2MetricFile>) -> anyhow::Result<K8SProbe> {
        let mut metric_counter: Vec<(CgroupV2MetricFile, CounterDiff, CounterDiff, CounterDiff)> = Vec::new();
        for metric_file in final_li_metric {
            //elm is  a CgroupV2MetricFile
            let counter_tmp_tot = CounterDiff::with_max_value(CGROUP_MAX_TIME_COUNTER);
            let counter_tmp_usr = CounterDiff::with_max_value(CGROUP_MAX_TIME_COUNTER);
            let counter_tmp_sys = CounterDiff::with_max_value(CGROUP_MAX_TIME_COUNTER);
            metric_counter.push((metric_file, counter_tmp_tot, counter_tmp_usr, counter_tmp_sys));
        }


        return Ok(K8SProbe {
            metrics: metric,
            metric_and_counter: metric_counter,
        });
    }

    pub fn add_entry(&mut self, metric_file: CgroupV2MetricFile) {
        let counter_tmp_tot = CounterDiff::with_max_value(CGROUP_MAX_TIME_COUNTER);
        let counter_tmp_usr = CounterDiff::with_max_value(CGROUP_MAX_TIME_COUNTER);
        let counter_tmp_sys = CounterDiff::with_max_value(CGROUP_MAX_TIME_COUNTER);
        self.metric_and_counter.push((metric_file, counter_tmp_tot, counter_tmp_usr, counter_tmp_sys));
    }

}

impl alumet::pipeline::Source for K8SProbe {
    fn poll(
        &mut self,
        measurements: &mut MeasurementAccumulator,
        timestamp: Timestamp,
    ) -> Result<(), alumet::pipeline::PollError> {
        let mut file_buffer = String::new();
        let mut element_unreachable: Vec<String> = Vec::new();
        for (metric_file, counter_tot, counter_usr, counter_sys) in &mut self.metric_and_counter {
            match cgroup_v2::gather_value(metric_file, &mut file_buffer){
                Ok(metrics_gathered) => {
                    let diff_tot = match counter_tot.update(metrics_gathered.time_used_tot) {
                        CounterDiffUpdate::FirstTime => None,
                        CounterDiffUpdate::Difference(diff) | CounterDiffUpdate::CorrectedDifference(diff) => Some(diff),
                    };
                    let diff_usr = match counter_usr.update(metrics_gathered.time_used_user_mode) {
                        CounterDiffUpdate::FirstTime => None,
                        CounterDiffUpdate::Difference(diff) => Some(diff),
                        CounterDiffUpdate::CorrectedDifference(diff) => Some(diff),
                    };
                    let diff_sys = match counter_sys.update(metrics_gathered.time_used_system_mode) {
                        CounterDiffUpdate::FirstTime => None,
                        CounterDiffUpdate::Difference(diff) => Some(diff),
                        CounterDiffUpdate::CorrectedDifference(diff) => Some(diff),
                    };
                    let consumer = ResourceConsumer::ControlGroup {
                        path: (metric_file.path.to_string_lossy().to_string().into()),
                    };
                    if let Some(value_tot) = diff_tot {
                        let p_tot: MeasurementPoint = MeasurementPoint::new(
                            timestamp,
                            self.metrics.time_used_tot,
                            Resource::LocalMachine,
                            consumer.clone(),
                            value_tot as u64,
                        )
                        .with_attr("pod", AttributeValue::String(metrics_gathered.name.clone()));
                        measurements.push(p_tot);
                    }
                    if let Some(value_usr) = diff_usr {
                        let p_usr: MeasurementPoint = MeasurementPoint::new(
                            timestamp,
                            self.metrics.time_used_user_mode,
                            Resource::LocalMachine,
                            consumer.clone(),
                            value_usr as u64,
                        )
                        .with_attr("pod", AttributeValue::String(metrics_gathered.name.clone()));
                        measurements.push(p_usr);
                    }
                    if let Some(value_sys) = diff_sys {
                        let p_sys: MeasurementPoint = MeasurementPoint::new(
                            timestamp,
                            self.metrics.time_used_system_mode,
                            Resource::LocalMachine,
                            consumer.clone(),
                            value_sys as u64,
                        )
                        .with_attr("pod", AttributeValue::String(metrics_gathered.name.clone()));
                        measurements.push(p_sys);
                    }
                },
                Err(_) => {
                    element_unreachable.push(metric_file.name.clone());
                }
            };
        }
        //Clean vector of all unreachable elements
        for to_delete in  element_unreachable.iter(){
            if let Some(index) = self.metric_and_counter.iter().position(|(metric_file, _, _, _)| metric_file.name == *to_delete) {
                self.metric_and_counter.swap_remove(index);
            }
        }

        let mut buffer = [0; 1024];
        let events = if let Ok(mut inotify_guard) = INOTIFY_VAR.lock() {
            if let Some(inotify) = inotify_guard.as_mut() {
                match inotify.read_events(&mut buffer) {
                    Ok(events) => Some(events),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => None,
                    Err(_) => panic!("Error while reading events"),
                }
            } else {
                None
            }
        } else {
            None
        };
        
        if let Some(events) = events {
            for event in events {
                // Example output: Event: Event { wd: WatchDescriptor { id: 1, fd: (Weak) }, mask: CREATE | ISDIR, cookie: 0, name: Some("test.slice") }
                if event.mask.contains(EventMask::CREATE | EventMask::ISDIR) {
                    let raw_name: &str = event.name.unwrap().to_str().expect("Error when converting event.name to str");
                    let name: String = raw_name.strip_suffix(".slice").unwrap_or(raw_name).to_owned();                
                    let map_guard = MAP_FD.lock().unwrap();
                    let tmp_path = map_guard.get(&event.wd.get_watch_descriptor_id()).unwrap();
                    let mut full_tmp_path: PathBuf = tmp_path.clone();
                    full_tmp_path.push(raw_name);
                    let mut final_path: PathBuf = full_tmp_path.clone();
                    final_path.push("cpu.stat");
                    let file_desc = File::open(&final_path).map_err(|_| format!("failed to open file {}", final_path.display())).unwrap();
                    let metric_file = CgroupV2MetricFile {
                        name: name.to_owned(),
                        path: full_tmp_path.to_path_buf(),
                        file: file_desc,
                    };
                    self.add_entry(metric_file)
                }
                if event.mask.contains(EventMask::DELETE | EventMask::ISDIR){
                    let raw_name: String = event.name.unwrap().to_str().expect("Error when converting event.name to str").to_owned();
                    if let Some(index) = self.metric_and_counter.iter().position(|(metric_file, _, _, _)| metric_file.name == *raw_name) {
                        println!("REMOVED: {:?}", raw_name);
                        self.metric_and_counter.swap_remove(index);
                    }
                }
            }
        }
        Ok(())
    }
}

impl Metrics {
    pub fn new(alumet: &mut AlumetStart) -> Result<Self, MetricCreationError> {
        let usec: PrefixedUnit = PrefixedUnit::micro(Unit::Second);
        Ok(Self {
            time_used_tot: alumet.create_metric::<u64>(
                "total_usage_usec",
                usec.clone(),
                "Total CPU usage time by the group",
            )?,
            time_used_user_mode: alumet.create_metric::<u64>(
                "user_usage_usec",
                usec.clone(),
                "User CPU usage time by the group",
            )?,
            time_used_system_mode: alumet.create_metric::<u64>(
                "system_usage_usec",
                usec.clone(),
                "System CPU usage time by the group",
            )?,
        })
    }
}
