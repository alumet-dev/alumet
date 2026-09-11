//! Native perf events: the built-in kernel tables (hardware, software, cache).
//!
//! This is one of the plugin's encoders: [`parse`] turns a symbolic name into a [`NamedPerfEvent`],
//! trying the hardware table, then the software table, then the compositional cache form.

use std::{error::Error, fmt::Display};

use anyhow::Context;
use itertools::Itertools;
use perf_event::events::{self, CacheId, CacheOp, CacheResult};
use perf_event_open_sys::bindings::{PERF_TYPE_HARDWARE, PERF_TYPE_HW_CACHE, PERF_TYPE_SOFTWARE};

use crate::spec::{EventEncoding, NamedPerfEvent};

#[derive(Debug)]
pub struct UnknownEventError;

impl Display for UnknownEventError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown event")
    }
}

impl Error for UnknownEventError {}

/// Resolve a native event name against the built-in kernel tables.
///
/// ## Example
/// ```ignore
/// let event = parse("REF_CPU_CYCLES").unwrap();
/// let event = parse("LL_READ_MISS").unwrap();
/// ```
pub fn parse(name: &str) -> anyhow::Result<NamedPerfEvent> {
    if let Ok(e) = parse_hardware(name) {
        return Ok(e);
    }
    if let Ok(e) = parse_software(name) {
        return Ok(e);
    }
    parse_cache(name)
}

/// Pin an already-resolved native event to a PMU of numeric type `pmu_type`.
/// Only hardware and hardware-cache events can be pinned.
pub(crate) fn pin(base: &NamedPerfEvent, pmu: &str, pmu_type: u32) -> anyhow::Result<EventEncoding> {
    match base.encoding.type_ {
        PERF_TYPE_HARDWARE | PERF_TYPE_HW_CACHE => Ok(EventEncoding {
            config: extended_config(base.encoding.config, pmu_type),
            ..base.encoding
        }),
        PERF_TYPE_SOFTWARE => anyhow::bail!(
            "software event '{}' is CPU-wide and cannot be pinned to PMU '{pmu}'",
            base.name
        ),
        other => anyhow::bail!("event '{}' (perf type {other}) cannot be pinned to a PMU", base.name),
    }
}

/// Pack a PMU's numeric type into the high 32 bits of `config`.
fn extended_config(config: u64, pmu_type: u32) -> u64 {
    config | (u64::from(pmu_type) << 32)
}

/// Returns an hardware perf event from its name.
///
/// ## Example
/// ```ignore
/// let event = parse_hardware("INSTRUCTIONS").unwrap();
/// ```
fn parse_hardware(event_name: &str) -> Result<NamedPerfEvent, UnknownEventError> {
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
        encoding: EventEncoding::from_event(event),
    })
}

/// Returns a software perf event from its name.
///
/// ## Example
/// ```ignore
/// let event = parse_software("CONTEXT_SWITCHES").unwrap();
/// ```
fn parse_software(event_name: &str) -> Result<NamedPerfEvent, UnknownEventError> {
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
        encoding: EventEncoding::from_event(event),
    })
}

/// Returns a cache perf event from a string of the form `<name>_<op>_<result>`.
///
/// ## Example
/// ```ignore
/// let event = parse_cache("L1D_READ_ACCESS").unwrap();
/// let event = parse_cache("LL_WRITE_MISS").unwrap();
/// ```
fn parse_cache(cache_spec: &str) -> anyhow::Result<NamedPerfEvent> {
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
        encoding: EventEncoding::from_event(events::Cache {
            which: cache_id,
            operation: cache_op,
            result: cache_result,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hardware_software_and_cache_parse() {
        assert_eq!(parse("INSTRUCTIONS").unwrap().name, "INSTRUCTIONS");
        assert_eq!(parse("CONTEXT_SWITCHES").unwrap().name, "CONTEXT_SWITCHES");
        assert_eq!(parse("LL_READ_MISS").unwrap().name, "LL_READ_MISS");
    }

    #[test]
    fn unknown_name_is_rejected() {
        assert!(parse("DEFINITELY_NOT_A_REAL_EVENT_XYZ").is_err());
    }

    #[test]
    fn extended_config_packs_pmu_type_high() {
        // (pmu_type << 32) | config. e.g. cpu_core (type 4) + generic INSTRUCTIONS (1).
        assert_eq!(extended_config(0x1, 4), 0x4_0000_0001);
        assert_eq!(extended_config(0xc0, 10), 0xa_0000_00c0);
    }

    #[test]
    fn software_cannot_be_pinned_to_a_pmu() {
        // Software events are CPU-wide; the type check rejects them (no sysfs read needed).
        let base = parse("CONTEXT_SWITCHES").unwrap();
        let err = pin(&base, "cpu_core", 4).unwrap_err();
        assert!(format!("{err:#}").contains("CPU-wide"), "got: {err:#}");
    }

    #[test]
    fn hardware_pinned_to_pmu_uses_extended_type() {
        // Pinning is pure: the generic type is preserved and the PMU's numeric type is packed into
        // config's high bits. cpu_core is type 4 on this ABI.
        let base = parse("INSTRUCTIONS").unwrap();
        let e = pin(&base, "cpu_core", 4).unwrap();
        assert_eq!(e.type_, base.encoding.type_);
        assert_eq!(e.config, extended_config(base.encoding.config, 4));
    }
}
