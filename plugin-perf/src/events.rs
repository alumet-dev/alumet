//! Names of performance events and utilities.

use std::{error::Error, fmt::Display};

use anyhow::Context;
use itertools::Itertools;
use perf_event::events::{self, CacheId, CacheOp, CacheResult};

#[derive(Clone)]
pub struct NamedPerfEvent<E: events::Event + Clone> {
    pub name: String,
    pub description: String,
    pub event: E,
}

impl<E: events::Event + Clone + From<u64>> NamedPerfEvent<E> {
    /// An event with a custom user-supplied id to pass to `perf_event_open`.
    pub fn custom(id: u64) -> Self {
        Self {
            name: format!("custom-{id}"),
            description: "?".to_owned(),
            event: E::from(id),
        }
    }
}

#[derive(Debug)]
pub struct UnknownEventError;

impl Display for UnknownEventError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown event")
    }
}

impl Error for UnknownEventError {}

/// Returns a hardware perf event from its name.
///
/// ## Example
/// ```ignore
/// let event = parse_hardware("REF_CPU_CYCLES").unwrap();
/// ```
pub fn parse_hardware(event_name: &str) -> Result<NamedPerfEvent<events::Hardware>, UnknownEventError> {
    let uppercase_name = event_name.to_ascii_uppercase();
    let (event, description) = match uppercase_name.as_ref() {
        "CPU_CYCLES" => Ok((events::Hardware::CPU_CYCLES, "Total cycles.")),
        "INSTRUCTIONS" => Ok((events::Hardware::INSTRUCTIONS, "Retired instructions")),
        "CACHE_REFERENCES" => Ok((events::Hardware::CACHE_REFERENCES, "Cache accesses")),
        "CACHE_MISSES" => Ok((events::Hardware::CACHE_MISSES, "Cache misses")),
        "BRANCH_INSTRUCTIONS" => Ok((events::Hardware::BRANCH_INSTRUCTIONS, "Retired branch instructions")),
        "BRANCH_MISSES" => Ok((events::Hardware::BRANCH_MISSES, "Mispredicted branch instructions")),
        "BUS_CYCLES" => Ok((events::Hardware::BUS_CYCLES, "Bus cycles")),
        "STALLED_CYCLES_FRONTEND" => Ok((events::Hardware::STALLED_CYCLES_FRONTEND, "Stalled cycles during issue")),
        "STALLED_CYCLES_BACKEND" => Ok((
            events::Hardware::STALLED_CYCLES_BACKEND,
            "Stalled cycles during retirement",
        )),
        "REF_CPU_CYCLES" => Ok((
            events::Hardware::REF_CPU_CYCLES,
            "Total cycles, independent of frequency scaling",
        )),
        _ => Err(UnknownEventError),
    }?;
    Ok(NamedPerfEvent {
        name: uppercase_name,
        description: description.to_owned(),
        event,
    })
}

/// Returns a software perf event from its name.
///
/// ## Example
/// ```ignore
/// let event = parse_software("CONTEXT_SWITCHES").unwrap();
/// ```
pub fn parse_software(event_name: &str) -> Result<NamedPerfEvent<events::Software>, UnknownEventError> {
    let uppercase_name = event_name.to_ascii_uppercase();
    // CPU_CLOCK and TASK_CLOCK are not supported here, because they require an additional parameter
    // (frequency or period) and because we don't need them for monitoring and profiling purposes.
    let (event, description) = match uppercase_name.as_ref() {
        "PAGE_FAULTS" => Ok((events::Software::PAGE_FAULTS, "Page faults.")),
        "CONTEXT_SWITCHES" => Ok((events::Software::CONTEXT_SWITCHES, "Context switches.")),
        "CPU_MIGRATIONS" => Ok((events::Software::CPU_MIGRATIONS, "Process migration to another CPU.")),
        "PAGE_FAULTS_MIN" => Ok((
            events::Software::PAGE_FAULTS_MIN,
            "Minor page faults: resolved without needing I/O.",
        )),
        "PAGE_FAULTS_MAJ" => Ok((
            events::Software::PAGE_FAULTS_MAJ,
            "Major page faults: I/O was required to resolve these.",
        )),
        "ALIGNMENT_FAULTS" => Ok((
            events::Software::ALIGNMENT_FAULTS,
            "Alignment faults that required kernel intervention.",
        )),
        "EMULATION_FAULTS" => Ok((events::Software::EMULATION_FAULTS, "Instruction emulation faults.")),
        // "DUMMY" => Ok((events::Software::DUMMY, "Placeholder.")),
        // "BPF_OUTPUT" => Ok((events::Software::DUMMY, "Placeholder.")),
        "CGROUP_SWITCHES" => Ok((
            events::Software::DUMMY,
            "Context switches to a task in a different cgroup.",
        )),
        _ => Err(UnknownEventError),
    }?;
    Ok(NamedPerfEvent {
        name: uppercase_name,
        description: description.to_owned(),
        event,
    })
}

/// Returns a cache perf event from a string of the form `<name>_<op>_<result>`.
///
/// ## Example
/// ```ignore
/// let event = parse_cache("L1D_READ_ACCESS").unwrap();
/// let event = parse_cache("LL_WRITE_MISS").unwrap();
/// ```
pub fn parse_cache(cache_spec: &str) -> anyhow::Result<NamedPerfEvent<events::Cache>> {
    let (name, op, result) = cache_spec
        .splitn(3, '_')
        .map(|s| s.to_ascii_uppercase())
        .collect_tuple()
        .context("invalid cache specification, expected <name>_<op>_<result>")?;

    let (cache_id, cache_id_desc) = match name.as_str() {
        "L1D" => Ok((CacheId::L1D, "Level 1 data cache")),
        "L1I" => Ok((CacheId::L1I, "Level 1 instruction cache")),
        "LL" => Ok((CacheId::LL, "Last-level cache")),
        "DTLB" => Ok((
            CacheId::DTLB,
            "Data translation lookaside buffer (virtual address translation)",
        )),
        "ITLB" => Ok((
            CacheId::ITLB,
            "Instruction translation lookaside buffer (virtual address translation)",
        )),
        "BPU" => Ok((CacheId::BPU, "Branch prediction.")),
        "NODE" => Ok((
            CacheId::NODE,
            "Memory accesses that stay local to the originating NUMA node",
        )),
        _ => Err(UnknownEventError),
    }
    .with_context(|| format!("invalid cache id {name}"))?;

    let (cache_op, cache_op_desc) = match op.as_str() {
        "READ" => Ok((CacheOp::READ, "read accesses")),
        "WRITE" => Ok((CacheOp::WRITE, "write accesses")),
        "PREFETCH" => Ok((CacheOp::PREFETCH, "prefetch accesses")),
        _ => Err(UnknownEventError),
    }
    .with_context(|| format!("invalid cache operation {name}"))?;

    let (cache_result, cache_result_desc) = match result.as_str() {
        "ACCESS" => Ok((CacheResult::ACCESS, "counting the number of cache accesses")),
        "MISS" => Ok((CacheResult::MISS, "counting the number of cache misses")),
        _ => Err(UnknownEventError),
    }
    .with_context(|| format!("invalid cache result {name}"))?;

    Ok(NamedPerfEvent {
        name: format!("{name}_{op}_{result}"),
        description: format!("{cache_id_desc}, {cache_op_desc}, {cache_result_desc}."),
        event: events::Cache {
            which: cache_id,
            operation: cache_op,
            result: cache_result,
        },
    })
}
