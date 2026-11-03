//! Workaround: counting a cgroup through its processes, for kernels that miscount multiplexed
//! cgroup events.
//!
//! Since Linux 6.2 (bd2756811766, RHEL 9.3), the cgroup events report a wrong `time_running` once
//! multiplexed, so their scaled values are wrong (fixed by e4c7c401c6b9 in Linux 7.3).
//! It's intended to be removed when most supported kernels have the fix.
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::counters::{Counters, GroupedEvent, Observed, Reading};
use crate::group::Accuracy;

/// How a process of the cgroup is counted.
enum ProcessCounting {
    /// By its own counters (with inherit), opened on every thread it had then.
    Own {
        boot_start: u64,
        boot_end: u64,
        threads: Vec<Counters>,
    },
    /// By the counters of an ancestor, as it was born after them.
    Inherited,
}

/// A list of events counted on a cgroup through its processes: the counters of every process
pub struct CgroupProcesses {
    dir: PathBuf,
    events: Vec<GroupedEvent>,
    auto_scale: bool,
    counting: HashMap<u32, ProcessCounting>,
    /// The length of a clock tick (the unit of starttime), in ns.
    tick_ns: u64,
}

impl CgroupProcesses {
    pub fn new(dir: PathBuf, events: Vec<GroupedEvent>, auto_scale: bool) -> Self {
        Self {
            dir,
            events,
            auto_scale,
            counting: HashMap::new(),
            tick_ns: tick_ns(),
        }
    }

    /// Opens the processes of the cgroup that are not counted yet. Returns `false` once the
    /// cgroup is gone.
    ///
    /// Call it often: what a new process does before it is found is lost.
    pub fn scan(&mut self) -> bool {
        let Some(pids) = cgroup_procs(&self.dir) else {
            return false;
        };
        let mut new: Vec<(u32, u32, u64)> = pids
            .into_iter()
            .filter(|p| !self.counting.contains_key(p))
            .filter_map(|p| stat_of(p).map(|(ppid, start)| (p, ppid, start)))
            .collect();
        // Parents before children.
        new.sort_by_key(|&(_, _, start)| start);
        let tick = self.tick_ns;
        for (pid, ppid, start) in new {
            // `starttime` is in clock ticks: the process was born in [start, start + 1) ticks.
            let inherited = match self.counting.get(&ppid) {
                Some(ProcessCounting::Inherited) => true,
                Some(ProcessCounting::Own { boot_end, .. }) if start * tick > *boot_end => true,
                Some(ProcessCounting::Own { boot_start, .. }) if (start + 1) * tick > *boot_start => {
                    // Born while its parent was being opened
                    log::debug!("process {pid} of {} may be counted twice", self.dir.display());
                    false
                }
                _ => false,
            };
            if inherited {
                self.counting.insert(pid, ProcessCounting::Inherited);
            } else {
                self.open_process(pid);
            }
        }
        true
    }

    /// Opens every thread of `pid`.
    ///
    /// A thread created meanwhile by a thread not opened yet is lost. Listing the threads again
    /// would not help: those created by an opened thread are inherited, and nothing tells which
    /// thread created which, so they would be counted twice.
    fn open_process(&mut self, pid: u32) {
        let boot_start = boottime_ns();
        let mut threads = Vec::new();
        for tid in threads_of(pid) {
            let observed = Observed::Task { tid, inherit: true };
            match Counters::open(&observed, &self.events, self.auto_scale) {
                Ok(c) => threads.push(c),
                // The thread exited meanwhile: nothing to count.
                Err(e) if is_no_such_task(&e) => (),
                Err(e) => {
                    log::warn!(
                        "cannot count thread {tid} of process {pid} in {}, will retry: {e:#}",
                        self.dir.display()
                    );
                    return;
                }
            }
        }
        if threads.is_empty() {
            return;
        }
        log::debug!(
            "counting process {pid} ({} threads) of {}",
            threads.len(),
            self.dir.display()
        );
        self.counting.insert(
            pid,
            ProcessCounting::Own {
                boot_start,
                boot_end: boottime_ns(),
                threads,
            },
        );
    }

    /// The values of every event counted (by index in the `events` given to [`new`](Self::new)),
    /// summed over every thread opened, with the worst accuracy. The processes that have exited
    /// keep their final values.
    pub fn read(&mut self) -> Vec<Reading> {
        let mut sums: Vec<Option<(u64, Accuracy)>> = vec![None; self.events.len()];
        let threads = self.counting.values_mut().flat_map(|c| match c {
            ProcessCounting::Own { threads, .. } => threads.iter_mut(),
            ProcessCounting::Inherited => [].iter_mut(),
        });
        for thread in threads {
            let readings = match thread.read() {
                Ok(readings) => readings,
                Err(e) => {
                    log::warn!("could not read the counters of a thread: {e:#}");
                    continue;
                }
            };
            for r in readings {
                let (sum, accuracy) = sums[r.event].get_or_insert((0, Accuracy::Exact));
                *sum += r.value;
                *accuracy = (*accuracy).max(r.accuracy);
            }
        }
        sums.into_iter()
            .enumerate()
            .filter_map(|(event, sum)| {
                sum.map(|(value, accuracy)| Reading {
                    event,
                    cpu: None,
                    value,
                    accuracy,
                })
            })
            .collect()
    }
}

/// `(ppid, starttime in clock ticks since boot)` from `/proc/<pid>/stat`.
fn stat_of(pid: u32) -> Option<(u32, u64)> {
    let s = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    parse_stat(&s)
}

/// The same, from the content of a `/proc/<pid>/stat` file.
fn parse_stat(stat: &str) -> Option<(u32, u64)> {
    // The comm field may contain spaces and parentheses: parse after the last ')'.
    let f: Vec<&str> = stat[stat.rfind(')')? + 2..].split(' ').collect();
    // f[0] is field 3 (state): ppid is field 4, starttime field 22.
    Some((f.get(1)?.parse().ok()?, f.get(19)?.parse().ok()?))
}

/// The processes of a cgroup and of its descendants, `None` once the cgroup is gone.
fn cgroup_procs(dir: &Path) -> Option<Vec<u32>> {
    fn walk(dir: &Path, pids: &mut Vec<u32>) -> Option<()> {
        let procs = fs::read_to_string(dir.join("cgroup.procs")).ok()?;
        pids.extend(parse_cgroup_procs(&procs));
        for e in fs::read_dir(dir).ok()?.flatten() {
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                // A sub-cgroup removed meanwhile is not an error.
                let _ = walk(&e.path(), pids);
            }
        }
        Some(())
    }
    let mut pids = Vec::new();
    walk(dir, &mut pids)?;
    Some(pids)
}

/// The pids listed in the content of a `cgroup.procs` file.
fn parse_cgroup_procs(procs: &str) -> impl Iterator<Item = u32> + '_ {
    procs.lines().filter_map(|l| l.parse().ok())
}

fn threads_of(pid: u32) -> Vec<u32> {
    fs::read_dir(format!("/proc/{pid}/task"))
        .map(|d| parse_thread_names(d.flatten().map(|e| e.file_name())))
        .unwrap_or_default()
}

/// The tids among the file names of a `/proc/<pid>/task` listing.
fn parse_thread_names(names: impl Iterator<Item = OsString>) -> Vec<u32> {
    names.filter_map(|n| n.to_str()?.parse().ok()).collect()
}

/// The clock of `starttime` in `/proc/<pid>/stat`.
fn boottime_ns() -> u64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut ts) };
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}

fn tick_ns() -> u64 {
    1_000_000_000 / unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as u64
}

fn is_no_such_task(e: &anyhow::Error) -> bool {
    e.chain()
        .filter_map(|c| c.downcast_ref::<io::Error>())
        .any(|e| e.raw_os_error() == Some(libc::ESRCH))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_stat() {
        let (ppid, start) = stat_of(std::process::id()).unwrap();
        assert!(ppid > 0 && start > 0);
    }

    #[test]
    fn stat_with_spaces_and_parentheses_in_comm() {
        // Fields after the last ')' of comm: state, ppid, ... 18 more ..., starttime.
        let stat = "1234 (my (weird) app) R 42 1234 1234 0 -1 4194560 100 0 5 0 900 10 0 0 20 0 7 0 1234567 0 0";
        assert_eq!(parse_stat(stat), Some((42, 1234567)));
    }

    #[test]
    fn stat_malformed() {
        assert_eq!(parse_stat("no parenthesis"), None);
        // Too few fields after comm.
        assert_eq!(parse_stat("1 (a) S 42 1 1 0 -1"), None, "starttime is missing");
        // ppid is not a number.
        assert_eq!(
            parse_stat(&format!("1 (a) S x 1 1 0 -1 0 0 0 0 0 0 0 0 0 20 0 1 0 {}", 7)),
            None
        );
    }

    #[test]
    fn cgroup_procs_parsing() {
        let pids: Vec<u32> = parse_cgroup_procs("1\n2\n3\n").collect();
        assert_eq!(pids, [1, 2, 3]);
        // A last line without '\n', a blank line and garbage are handled.
        let pids: Vec<u32> = parse_cgroup_procs("42\n\nnot-a-pid\n7").collect();
        assert_eq!(pids, [42, 7]);
        assert_eq!(parse_cgroup_procs("").count(), 0);
    }

    #[test]
    fn thread_names_parsing() {
        let names = ["100", "101", "self"].map(OsString::from).into_iter();
        assert_eq!(parse_thread_names(names), [100, 101]);
        assert!(parse_thread_names(std::iter::empty()).is_empty());
    }
}
