//! Unified perf event descriptor.
//!
//! An event is written `<event>[#<modifiers>]`.
//!
//! `<event>` can take one of these forms (a bit like `perf stat -e`):
//!
//! - **native** : a symbolic event name (`INSTRUCTIONS`, `LL_READ_MISS`), encoded from the native
//!   kernel tables (hardware/software/cache). On a hybrid CPU, a generic
//!   hardware/cache event is counted on *every* core PMU (`cpu_core` + `cpu_atom`), like `perf stat`
//!   does; the resulting measurements share the metric name and carry a `pmu` attribute.
//! - **native on a PMU** : a native name pinned to a PMU, `pmu/NAME` (e.g. `cpu_core/INSTRUCTIONS`).
//! - **libpfm** : any other name, optionally with unit masks (e.g. `RESOURCE_STALLS:ANY`), resolved
//!   through libpfm (per-CPU encoding tables). The fallback when the native tables don't know the
//!   name.
//! - **raw-hex** : a raw code `rN` (hex register encoding) on the default raw PMU.
//! - **raw on a PMU** : `pmu/rN`, the same code on a named PMU.
//! - **pmu-named** : `pmu/event=M,umask=N,…/`, using the named fields from
//!   `/sys/bus/event_source/devices/<pmu>/format/*`. **Not yet supported** (rejected with a clear
//!   "planned for a future release" error).
//!
//! An event's [`Scope`] (task-attached vs system-wide) is then derived from the PMU it targets; see
//! [`Scope`] and [`crate::source`].

use alumet::resources::Resource;
use anyhow::Context;
use perf_event::events::Event;
use perf_event_open_sys::bindings::{
    PERF_TYPE_HARDWARE, PERF_TYPE_HW_CACHE, PERF_TYPE_RAW, PERF_TYPE_SOFTWARE, perf_event_attr,
};
use serde::{Deserialize, Serialize};

use crate::cpu;
use crate::native;
use crate::pfm;
use crate::pmu;
use crate::raw;

/// One entry of the `events` config list: a bare string, or a table with a metric `rename`.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum EventEntry {
    Simple(String),
    Detailed {
        event: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rename: Option<String>,
    },
}

impl EventEntry {
    fn parts(&self) -> (&str, Option<&str>) {
        match self {
            EventEntry::Simple(s) => (s, None),
            EventEntry::Detailed { event, rename } => (event, rename.as_deref()),
        }
    }
}

/// perf event domain modifiers, e.g. `INSTRUCTIONS#u:k`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Modifiers {
    user: bool,
    kernel: bool,
    hv: bool,
    host: bool,
    guest: bool,
    idle_only: bool,
}

impl Modifiers {
    /// Parse the modifiers that follow the `#` delimiter, `:` separated.
    /// They can be combined (e.g. `#u:k`).
    /// An unknown or empty token is rejected.
    fn parse(s: &str) -> anyhow::Result<Self> {
        let mut m = Modifiers::default();
        if s.is_empty() {
            return Ok(m);
        }
        for token in s.split(':') {
            match token {
                "u" => m.user = true,
                "k" => m.kernel = true,
                "h" => m.hv = true,
                "H" => m.host = true,
                "G" => m.guest = true,
                "I" => m.idle_only = true,
                "" => anyhow::bail!("empty modifier (check the ':' separators)"),
                other => anyhow::bail!("unknown modifier '{other}'"),
            }
        }
        Ok(m)
    }

    fn any(&self) -> bool {
        self.user || self.kernel || self.hv || self.host || self.guest || self.idle_only
    }

    /// The `exclude_*` bits these modifiers produce.
    ///
    /// With no domain modifier we keep the original plugin's default **user space only** (kernel
    /// and hypervisor excluded). A domain modifier restricts to the listed domains by excluding the others.
    fn excludes(&self) -> Excludes {
        let (user, kernel, hv) = if self.user || self.kernel || self.hv {
            (!self.user, !self.kernel, !self.hv)
        } else {
            (false, true, true)
        };
        Excludes {
            user,
            kernel,
            hv,
            host: self.guest && !self.host,  // guest-only excludes the host
            guest: self.host && !self.guest, // host-only excludes the guest
            idle: self.idle_only,
        }
    }

    /// Apply the modifiers to a builder. This must run *after* [`perf_event::Builder::new`], which
    /// forces its own `exclude_kernel`/`exclude_hv` defaults; we set every bit explicitly so the
    /// result never depends on that ordering.
    fn configure(&self, builder: &mut perf_event::Builder<'_>) {
        let e = self.excludes();
        builder
            .exclude_user(e.user)
            .exclude_kernel(e.kernel)
            .exclude_hv(e.hv)
            .exclude_host(e.host)
            .exclude_guest(e.guest)
            .exclude_idle(e.idle);
    }
}

/// The `exclude_*` bits computed from a [`Modifiers`] set (`true` = the domain is *not* measured).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Excludes {
    user: bool,
    kernel: bool,
    hv: bool,
    host: bool,
    guest: bool,
    idle: bool,
}

/// A perf event reduced to the four `perf_event_attr` fields every encoder ultimately writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EventEncoding {
    pub(crate) type_: u32,
    pub(crate) config: u64,
    pub(crate) config1: u64,
    pub(crate) config2: u64,
}

impl EventEncoding {
    pub(crate) fn from_event(event: impl Event) -> Self {
        let mut attr = perf_event_attr::default();
        event.update_attrs(&mut attr);
        Self::from_attr(&attr)
    }

    /// Read the four encoding fields out of an already-filled `perf_event_attr`.
    pub(crate) fn from_attr(attr: &perf_event_attr) -> Self {
        Self {
            type_: attr.type_,
            config: attr.config,
            config1: attr.config1,
            config2: attr.config2,
        }
    }
}

impl Event for EventEncoding {
    fn update_attrs(self, attr: &mut perf_event_attr) {
        attr.type_ = self.type_;
        attr.config = self.config;
        attr.config1 = self.config1;
        attr.config2 = self.config2;
    }
}

/// An encoded event together with its canonical name and description.
/// This is what encoders returns.
#[derive(Debug, Clone)]
pub(crate) struct NamedPerfEvent {
    pub name: String,
    pub description: String,
    pub encoding: EventEncoding,
}

/// A fully-configured event.
/// This is what's added to a perf group.
#[derive(Debug, Clone)]
pub struct ConfiguredEvent {
    encoding: EventEncoding,
    modifiers: Modifiers,
}

/// How an event must be opened, decided by the PMU it targets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    TaskAttached { binding: Option<CoreBinding> },
    SystemWide { pmu: String, cpus: Vec<u32> },
}

/// The core PMU a task-attached event is bound to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreBinding {
    pub pmu: String,
    pub cpus: Vec<u32>,
    pub resource: Resource,
}

/// A parsed config event: the metric name suffix (after `perf_`), a description and the event.
/// This is what's used by Alumet to setup metrics.
#[derive(Debug)]
pub struct ParsedEvent {
    pub metric_suffix: String,
    pub description: String,
    pub scope: Scope,
    pub event: ConfiguredEvent,
}

impl ConfiguredEvent {
    /// The raw encoding to hand to [`perf_event::Builder::new`]. [`EventEncoding`] is the only
    /// [`Event`] in the plugin; the modifiers are applied separately via [`Self::configure`].
    pub(crate) fn encoding(&self) -> EventEncoding {
        self.encoding
    }

    /// The key identifying which perf group this event may share. A perf group cannot span two
    /// different hardware PMUs, so [`crate::source`] groups events by this key.
    pub(crate) fn pmu_group_key(&self) -> u64 {
        let core = u64::from(PERF_TYPE_RAW);
        match self.encoding.type_ {
            PERF_TYPE_HARDWARE | PERF_TYPE_HW_CACHE => {
                let extended = self.encoding.config >> 32; // extended hardware type (hybrid pinning)
                if extended != 0 { extended } else { core }
            }
            PERF_TYPE_SOFTWARE => core, // software events can share any hardware group
            other => u64::from(other),  // RAW (== core), or a named PMU's dynamic type (raw-on-pmu)
        }
    }

    /// Apply this event's modifiers to a freshly-created builder. Must run *after*
    /// [`perf_event::Builder::new`], which forces its own `exclude_kernel`/`exclude_hv` defaults;
    /// this sets every bit explicitly so the result never depends on that ordering.
    pub fn configure(&self, builder: &mut perf_event::Builder<'_>) {
        self.modifiers.configure(builder);
    }
}

/// Parse one config entry into one or more [`ParsedEvent`]s.
pub fn parse(entry: &EventEntry) -> anyhow::Result<Vec<ParsedEvent>> {
    let (input, rename) = entry.parts();
    // The `#` delimiter separates the encoder name from the plugin's modifiers.
    let (name, mods_str) = input.split_once('#').unwrap_or((input, ""));
    if name.is_empty() {
        anyhow::bail!("empty event name in '{input}'");
    }
    let modifiers = Modifiers::parse(mods_str).with_context(|| format!("invalid event '{input}'"))?;

    // A named PMU exposing a `cpumask` (uncore, power, cstate) is opened system-wide.
    if let Some((pmu_name, cpus)) = detect_system_wide(name).with_context(|| format!("invalid event '{input}'"))? {
        if modifiers.any() {
            anyhow::bail!(
                "invalid event '{input}': modifiers do not apply to system-wide events (uncore/power/cstate)"
            );
        }
        let (base, _) = resolve_base(name).with_context(|| format!("invalid event '{input}'"))?;
        return Ok(vec![ParsedEvent {
            metric_suffix: sanitize(rename.unwrap_or(&base.name)),
            description: base.description,
            scope: Scope::SystemWide { pmu: pmu_name, cpus },
            event: ConfiguredEvent {
                encoding: base.encoding,
                modifiers,
            },
        }]);
    }

    resolve_task_attached(name, rename, modifiers).with_context(|| format!("invalid event '{input}'"))
}

fn detect_system_wide(name: &str) -> anyhow::Result<Option<(String, Vec<u32>)>> {
    match pmu::split(name) {
        Some((pmu, _terms)) => Ok(pmu::read_cpumask(pmu)?.map(|cpus| (pmu.to_owned(), cpus))),
        None => Ok(None),
    }
}

fn resolve_task_attached(name: &str, rename: Option<&str>, modifiers: Modifiers) -> anyhow::Result<Vec<ParsedEvent>> {
    let (base, placement) = resolve_base(name)?;
    let suffix = sanitize(rename.unwrap_or(&base.name));

    let make = |encoding: EventEncoding, binding: Option<CoreBinding>| ParsedEvent {
        metric_suffix: suffix.clone(),
        description: base.description.clone(),
        scope: Scope::TaskAttached { binding },
        event: ConfiguredEvent { encoding, modifiers },
    };

    match placement {
        // A raw code (`rN`), a software event, or a libpfm event: no specific PMU.
        Placement::Unpinned => Ok(vec![make(base.encoding, None)]),
        // An explicit `pmu/…`: bound to that one core PMU (its encoding is already pinned).
        Placement::PmuPinned(pmu) => Ok(vec![make(base.encoding, Some(core_binding(&pmu)?))]),
        // A bare generic event: counted on every core PMU (both clusters of a hybrid CPU).
        Placement::CoreGeneric => core_targets()?
            .into_iter()
            .map(|t| {
                let encoding = match t.pmu_type {
                    Some(pmu_type) => native::pin(&base, &t.binding.pmu, pmu_type)?,
                    None => base.encoding,
                };
                Ok(make(encoding, Some(t.binding)))
            })
            .collect(),
    }
}

/// How a task-attached event places onto the core PMU(s).
enum Placement {
    /// No specific PMU (raw code, software, or libpfm).
    Unpinned,
    /// An explicit core PMU.
    PmuPinned(String),
    /// A bare native hardware/cache event, to be counted on every core PMU.
    CoreGeneric,
}

/// Resolve an event name (no modifiers) into its encoding and how it places onto the core PMU(s).
/// Each encoder builds the [`NamedPerfEvent`] in its own module; this dispatches to them.
fn resolve_base(name: &str) -> anyhow::Result<(NamedPerfEvent, Placement)> {
    // `pmu/…`: a raw code or a native event pinned to that PMU (`cpu_core/INSTRUCTIONS`).
    if let Some((pmu_name, term)) = pmu::split(name) {
        if let Some(result) = raw::parse(name) {
            return Ok((result?, Placement::PmuPinned(pmu_name.to_owned())));
        }
        if let Ok(base) = native::parse(term) {
            let pmu_type = pmu::read_type(pmu_name)?;
            let encoding = native::pin(&base, pmu_name, pmu_type)?;
            return Ok((
                NamedPerfEvent { encoding, ..base },
                Placement::PmuPinned(pmu_name.to_owned()),
            ));
        }
        anyhow::bail!(
            "PMU events with named fields (`{name}`, i.e. `pmu/event=,umask=/`) are not supported yet; this is planned for a future release"
        );
    }

    // `rN`: a raw code on the default raw PMU.
    if let Some(result) = raw::parse(name) {
        return Ok((result?, Placement::Unpinned));
    }
    // Built-in kernel tables. Only generic hardware/cache events fan out; software events are
    // CPU-wide and stay unpinned.
    if let Ok(base) = native::parse(name) {
        use perf_event_open_sys::bindings::{PERF_TYPE_HARDWARE, PERF_TYPE_HW_CACHE};
        let placement = match base.encoding.type_ {
            PERF_TYPE_HARDWARE | PERF_TYPE_HW_CACHE => Placement::CoreGeneric,
            _ => Placement::Unpinned,
        };
        return Ok((base, placement));
    }
    // Fall back to libpfm.
    let base = pfm::encode(name)
        .with_context(|| format!("unknown event '{name}': not a native event, and libpfm could not encode it"))?;
    Ok((base, Placement::Unpinned))
}

struct CoreTarget {
    pmu_type: Option<u32>,
    binding: CoreBinding,
}

fn core_targets() -> anyhow::Result<Vec<CoreTarget>> {
    let cores = pmu::core_pmus()?;
    if cores.is_empty() {
        let cpus = cpu::online_cpus()?;
        let resource = package_resource(pmu::single_package(&cpus));
        return Ok(vec![CoreTarget {
            pmu_type: None,
            binding: CoreBinding {
                pmu: "cpu".to_owned(),
                cpus,
                resource,
            },
        }]);
    }
    Ok(cores
        .into_iter()
        .map(|c| CoreTarget {
            pmu_type: Some(c.type_),
            binding: CoreBinding {
                pmu: c.name,
                resource: package_resource(c.package),
                cpus: c.cpus,
            },
        })
        .collect())
}

fn core_binding(pmu: &str) -> anyhow::Result<CoreBinding> {
    let cpus = match pmu::read_cpus(pmu)? {
        Some(cpus) => cpus,
        None => cpu::online_cpus()?,
    };
    let resource = package_resource(pmu::single_package(&cpus));
    Ok(CoreBinding {
        pmu: pmu.to_owned(),
        cpus,
        resource,
    })
}

fn package_resource(package: Option<u32>) -> Resource {
    match package {
        Some(id) => Resource::CpuPackage { id },
        None => Resource::LocalMachine,
    }
}

/// Turn a string into a metric-name-safe suffix: letters are lowercased, non-alphanumeric
/// characters become `_`, and leading/trailing `_` are trimmed.
pub(crate) fn sanitize(s: &str) -> String {
    let mapped: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    mapped.trim_matches('_').to_owned()
}

#[cfg(test)]
mod tests {
    use perf_event::events::{Cache, CacheId, CacheOp, CacheResult, Hardware, Software};

    use super::*;

    fn parse_all(s: &str) -> Vec<ParsedEvent> {
        parse(&EventEntry::Simple(s.to_owned())).unwrap()
    }

    fn parse_one(s: &str) -> ParsedEvent {
        let mut evs = parse_all(s);
        assert_eq!(evs.len(), 1, "expected a single event for '{s}', got {}", evs.len());
        evs.pop().unwrap()
    }

    fn parse_first(s: &str) -> ParsedEvent {
        parse_all(s).into_iter().next().expect("at least one event")
    }

    fn is_hybrid() -> bool {
        pmu::read_type("cpu_core").is_ok() && pmu::read_type("cpu_atom").is_ok()
    }

    fn assert_generic(enc: &EventEncoding, base: &EventEncoding) {
        assert_eq!(enc.type_, base.type_);
        assert_eq!(enc.config & 0xffff_ffff, base.config & 0xffff_ffff);
        assert_eq!(enc.config1, base.config1);
        assert_eq!(enc.config2, base.config2);
    }

    #[test]
    fn native_hardware() {
        let base = EventEncoding::from_event(Hardware::REF_CPU_CYCLES);
        let evs = parse_all("REF_CPU_CYCLES");
        assert!(!evs.is_empty());
        for e in &evs {
            assert_eq!(e.metric_suffix, "ref_cpu_cycles");
            assert!(matches!(e.scope, Scope::TaskAttached { .. }));
            assert_generic(&e.event.encoding, &base);
        }
    }

    #[test]
    fn native_software() {
        let e = parse_one("CONTEXT_SWITCHES");
        assert_eq!(e.metric_suffix, "context_switches");
        assert_eq!(e.event.encoding, EventEncoding::from_event(Software::CONTEXT_SWITCHES));
        assert_eq!(e.scope, Scope::TaskAttached { binding: None });
    }

    #[test]
    fn native_cache() {
        let base = EventEncoding::from_event(Cache {
            which: CacheId::LL,
            operation: CacheOp::READ,
            result: CacheResult::MISS,
        });
        let evs = parse_all("LL_READ_MISS");
        assert!(!evs.is_empty());
        for e in &evs {
            assert_eq!(e.metric_suffix, "ll_read_miss");
            assert_generic(&e.event.encoding, &base);
        }
    }

    #[test]
    fn no_modifier_is_user_space_only() {
        // The default must match the original plugin: user space only (kernel + hv excluded).
        let e = parse_first("INSTRUCTIONS");
        assert_eq!(e.metric_suffix, "instructions");
        assert_eq!(
            e.event.modifiers.excludes(),
            Excludes {
                user: false,
                kernel: true,
                hv: true,
                host: false,
                guest: false,
                idle: false
            }
        );
    }

    #[test]
    fn user_modifier_matches_default() {
        let x = parse_first("INSTRUCTIONS#u").event.modifiers.excludes();
        assert!(!x.user && x.kernel && x.hv);
    }

    #[test]
    fn user_and_kernel_modifier() {
        // `#u:k` measures user and kernel, but still excludes the hypervisor.
        let x = parse_first("INSTRUCTIONS#u:k").event.modifiers.excludes();
        assert!(!x.user);
        assert!(!x.kernel);
        assert!(x.hv);
    }

    #[test]
    fn modifiers_must_be_colon_separated() {
        // Modifiers are `:`-separated tokens; the grouped form `#uk` is rejected.
        assert!(parse(&EventEntry::Simple("INSTRUCTIONS#uk".to_owned())).is_err());
        let x = parse_first("INSTRUCTIONS#u:k").event.modifiers.excludes();
        assert!(!x.user && !x.kernel && x.hv);
    }

    #[test]
    fn kernel_only_modifier() {
        // `#k` measures kernel only: user is excluded, kernel is counted.
        let x = parse_first("INSTRUCTIONS#k").event.modifiers.excludes();
        assert!(x.user);
        assert!(!x.kernel);
        assert!(x.hv);
    }

    #[test]
    fn host_and_idle_modifiers() {
        let x = parse_first("INSTRUCTIONS#H:I").event.modifiers.excludes();
        assert!(x.guest); // host only -> exclude guest
        assert!(!x.host);
        assert!(x.idle); // exclude idle
    }

    #[test]
    fn unknown_modifier_is_rejected() {
        // After `#`, everything is strictly a modifier, so a bad letter is a clear error.
        let err = parse(&EventEntry::Simple("INSTRUCTIONS#z".to_owned())).unwrap_err();
        assert!(format!("{err:#}").contains("unknown modifier"), "got: {err:#}");
    }

    #[test]
    fn hash_is_the_modifier_delimiter() {
        // Only what follows `#` is parsed as modifiers; the name is resolved untouched. The `#` is
        // stripped and never becomes part of the metric name.
        let base = EventEncoding::from_event(Hardware::INSTRUCTIONS);
        let e = parse_first("INSTRUCTIONS#u");
        assert_eq!(e.metric_suffix, "instructions");
        assert_generic(&e.event.encoding, &base);
        assert!(!e.event.modifiers.excludes().user);
    }

    #[test]
    fn rename_overrides_metric_name() {
        let e = parse(&EventEntry::Detailed {
            event: "LL_READ_MISS".to_owned(),
            rename: Some("my llc miss".to_owned()),
        })
        .unwrap();
        for parsed in &e {
            assert_eq!(parsed.metric_suffix, "my_llc_miss");
        }
    }

    #[test]
    fn events_without_a_pmu_are_task_attached() {
        assert!(
            parse_all("INSTRUCTIONS")
                .iter()
                .all(|e| matches!(e.scope, Scope::TaskAttached { .. }))
        );
        assert_eq!(parse_one("r0x412e").scope, Scope::TaskAttached { binding: None });
    }

    #[test]
    fn a_bare_generic_event_fans_out_on_a_hybrid_cpu() {
        if !is_hybrid() {
            eprintln!("skipping a_bare_generic_event_fans_out_on_a_hybrid_cpu: not a hybrid CPU");
            return;
        }
        let base = EventEncoding::from_event(Hardware::INSTRUCTIONS);
        let evs = parse_all("INSTRUCTIONS");
        let pmus: Vec<&str> = evs
            .iter()
            .map(|e| match &e.scope {
                Scope::TaskAttached { binding: Some(b) } => b.pmu.as_str(),
                other => panic!("expected a bound task-attached event, got {other:?}"),
            })
            .collect();
        assert!(pmus.contains(&"cpu_core") && pmus.contains(&"cpu_atom"), "got {pmus:?}");
        for e in &evs {
            assert_eq!(e.metric_suffix, "instructions");
            assert_generic(&e.event.encoding, &base);
            // Pinned to a specific cluster => the extended hardware type is set.
            assert_ne!(e.event.encoding.config >> 32, 0);
        }
    }

    #[test]
    fn distinct_core_pmus_get_distinct_group_keys() {
        if !is_hybrid() {
            eprintln!("skipping distinct_core_pmus_get_distinct_group_keys: not a hybrid CPU");
            return;
        }
        let core = parse_one("cpu_core/r0x1").event.pmu_group_key();
        let atom = parse_one("cpu_atom/r0x1").event.pmu_group_key();
        assert_ne!(core, atom);
    }

    #[test]
    fn a_system_wide_pmu_event_is_scoped_system_wide() {
        let Some((pmu, cpus)) = first_system_wide_pmu() else {
            eprintln!("skipping a_system_wide_pmu_event_is_scoped_system_wide: no system-wide PMU found");
            return;
        };
        let e = parse_one(&format!("{pmu}/r0x1"));
        assert_eq!(e.scope, Scope::SystemWide { pmu, cpus });
    }

    /// Find any PMU exposing a `cpumask` in sysfs (a system-wide PMU), returning its name and cpus.
    fn first_system_wide_pmu() -> Option<(String, Vec<u32>)> {
        let entries = std::fs::read_dir("/sys/bus/event_source/devices").ok()?;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Ok(Some(cpus)) = pmu::read_cpumask(&name) {
                return Some((name, cpus));
            }
        }
        None
    }

    #[test]
    fn raw_hex_event() {
        use perf_event_open_sys::bindings::PERF_TYPE_RAW;
        let e = parse_one("r0x412e#u:k");
        assert_eq!(e.metric_suffix, "r0x412e");
        assert_eq!(
            e.event.encoding(),
            EventEncoding {
                type_: PERF_TYPE_RAW,
                config: 0x412e,
                config1: 0,
                config2: 0,
            }
        );
        assert!(!e.event.modifiers.excludes().kernel);
    }

    #[test]
    fn pmu_term_rejected_for_now() {
        let err = parse(&EventEntry::Simple("cpu/event=0x2e,umask=0x41/".to_owned())).unwrap_err();
        assert!(format!("{err:#}").contains("future release"), "got: {err:#}");
    }

    #[test]
    fn unknown_name_mentions_libpfm() {
        // A name neither native nor encodable by libpfm fails, and the error names libpfm. Uses a
        // clearly-bogus name so it fails whether or not libpfm is installed.
        let err = parse(&EventEntry::Simple("DEFINITELY_NOT_A_REAL_EVENT_XYZ".to_owned())).unwrap_err();
        assert!(format!("{err:#}").contains("libpfm"), "got: {err:#}");
    }

    #[test]
    fn libpfm_event_resolves_when_available() {
        // Needs libpfm at runtime; skip cleanly where it is not installed.
        if pfm::encode("PERF_COUNT_HW_INSTRUCTIONS").is_err() {
            eprintln!("skipping libpfm_event_resolves_when_available: libpfm is not available");
            return;
        }
        // A generic name unknown to the native tables is resolved through libpfm (unpinned, never
        // fanned), and produces the same encoding as calling libpfm directly.
        let e = parse_one("PERF_COUNT_HW_INSTRUCTIONS");
        assert_eq!(e.metric_suffix, "perf_count_hw_instructions");
        assert_eq!(
            e.event.encoding,
            pfm::encode("PERF_COUNT_HW_INSTRUCTIONS").unwrap().encoding
        );
    }

    #[test]
    fn empty_is_rejected() {
        assert!(parse(&EventEntry::Simple(":u".to_owned())).is_err());
    }

    #[test]
    fn config_list_deserializes_mixed_entries() {
        // TOML 1.0 allows mixed-type arrays: bare strings and inline tables in the same list.
        #[derive(serde::Deserialize)]
        struct Wrap {
            events: Vec<EventEntry>,
        }
        let toml = r#"
            events = [
                "INSTRUCTIONS",
                "LL_READ_MISS",
                { event = "CACHE_MISSES", rename = "my_event" },
            ]
        "#;
        let w: Wrap = toml::from_str(toml).unwrap();
        assert_eq!(w.events.len(), 3);
        assert!(matches!(w.events[0], EventEntry::Simple(_)));
        assert!(matches!(w.events[2], EventEntry::Detailed { .. }));
    }
}
