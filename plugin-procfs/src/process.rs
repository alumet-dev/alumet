//! Process-level (by pid) metrics.

use std::collections::HashMap;

use alumet::{
    measurement::{MeasurementAccumulator, MeasurementBuffer, MeasurementPoint, Timestamp},
    metrics::TypedMetricId,
    pipeline::{elements::error::PollError, Source},
};
use anyhow::Context;
use procfs::{self, process::Process, ProcError, WithCurrentSystemInfo};

fn a() {
    // TODO DiskStat et IoPressure
    // procfs::process::all_processes()
}

struct MyProbe {
    metric: TypedMetricId<u64>,
}

impl Source for MyProbe {
    fn poll(&mut self, buffer: &mut MeasurementAccumulator, t: Timestamp) -> Result<(), PollError> {
        // let value = read_sensor();
        // buffer.push(MeasurementPoint::new(metric, value, ...));
        Ok(())
    }
}

/// Detects new processes.
///
/// The difficulty here is that process ids (PIDs) are not unique:
/// the kernel will reuse the ids after some time.
/// In case of a PID reuse, the file descriptor that pointed to the
/// old process is no longer valid. However, we pass the `Process`
/// (which contains the fd) to another task, and cannot check that
/// it still work in ProcessWatcher. `ProcessWatcher` implements
/// another heuristic.
struct ProcessWatcher {
    watched_processes: HashMap<i32, ProcessFingerprint>,
}

#[derive(PartialEq, Eq)]
struct ProcessFingerprint {
    start_time: u64,
    ppid: i32,
}

impl ProcessFingerprint {
    fn new(stat: &procfs::process::Stat) -> Self {
        Self {
            start_time: stat.starttime,
            ppid: stat.ppid,
        }
    }
}

impl ProcessWatcher {
    pub fn refresh(&mut self) -> anyhow::Result<()> {
        for p in procfs::process::all_processes().context("cannot read /proc")? {
            match p {
                Ok(process) => {
                    let pid = process.pid;
                    let stat = match process.stat() {
                        Err(ProcError::NotFound(_)) => continue, // process vanished, ignore
                        other => other,
                    }?;
                    let fingerprint = ProcessFingerprint::new(&stat);
                    let is_new = if let Some(existing) = self.watched_processes.get_mut(&pid) {
                        // A process with the same pid is in our HashMap.
                        // Is it the *same* process?
                        if *existing == fingerprint {
                            // same process, nothing to do
                            false
                        } else {
                            // new process! watch it
                            *existing = fingerprint;
                            true
                        }
                    } else {
                        self.watched_processes.insert(pid, fingerprint);
                        true
                    };
                    if is_new {
                        self.on_new_process(process)?;
                    }
                }
                Err(ProcError::NotFound(_)) => continue,
                Err(e) => Err(e)?,
            }
        }
        Ok(())
    }

    fn on_new_process(&mut self, p: Process) -> anyhow::Result<()> {
        todo!()
    }
}
