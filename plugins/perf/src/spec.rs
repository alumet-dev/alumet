//! Unified perf event descriptor.
//!
//! An event is written `<event>[#<modifiers>]`.
//!
//! `<event>` can take one of five forms (a bit like `perf stat -e`). We support a subset for
//! now and *recognise* the rest so the syntax stays stable across releases:
//!
//! - **native** : a symbolic event name (`INSTRUCTIONS`, `LL_READ_MISS`), encoded from the native
//!   kernel tables (hardware/software/cache). **Supported**.
//! - **libpfm** : any other name, optionally with unit masks (e.g. `RESOURCE_STALLS:ANY`), resolved
//!   through libpfm (per-CPU encoding tables). This is the fallback when the native tables don't
//!   know the name. **Supported**.
//! - **raw-hex** : a raw code `rN`, where `N` is a hexadecimal register encoding (layout from
//!   `/sys/bus/event_source/devices/cpu/format/*`). **Not yet supported** (planned).
//! - **pmu-named** : `pmu/event=M,umask=N,…/`, using named fields from
//!   `/sys/bus/event_source/devices/<pmu>/format/*` (also the uncore/`percore` qualifiers).
//!   **Not yet supported** (planned).
//! - **pmu-raw** : `pmu/config=M,config1=N,config2=K/`, the raw config registers given directly.
//!   **Not yet supported** (planned).
//!
//! A not-yet-supported form is rejected there with an explicit "planned for a future release" error.

use anyhow::Context;
use perf_event::events::Event;
use perf_event_open_sys::bindings::perf_event_attr;
use serde::{Deserialize, Serialize};

use crate::native;
use crate::pfm;

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

/// A parsed config event: the metric name suffix (after `perf_`), a description and the event.
/// This is what's used by Alumet to setup metrics.
#[derive(Debug)]
pub struct ParsedEvent {
    pub metric_suffix: String,
    pub description: String,
    pub event: ConfiguredEvent,
}

impl ConfiguredEvent {
    /// The raw encoding to hand to [`perf_event::Builder::new`]. [`EventEncoding`] is the only
    /// [`Event`] in the plugin; the modifiers are applied separately via [`Self::configure`].
    pub(crate) fn encoding(&self) -> EventEncoding {
        self.encoding
    }

    /// Apply this event's modifiers to a freshly-created builder. Must run *after*
    /// [`perf_event::Builder::new`], which forces its own `exclude_kernel`/`exclude_hv` defaults;
    /// this sets every bit explicitly so the result never depends on that ordering.
    pub fn configure(&self, builder: &mut perf_event::Builder<'_>) {
        self.modifiers.configure(builder);
    }
}

/// Parse one config entry into a [`ParsedEvent`].
pub fn parse(entry: &EventEntry) -> anyhow::Result<ParsedEvent> {
    let (input, rename) = entry.parts();
    // The `#` delimiter separates the encoder name from the plugin's modifiers.
    let (name, mods_str) = input.split_once('#').unwrap_or((input, ""));
    if name.is_empty() {
        anyhow::bail!("empty event name in '{input}'");
    }

    let modifiers = Modifiers::parse(mods_str).with_context(|| format!("invalid event '{input}'"))?;
    let resolved = resolve_event(name).with_context(|| format!("invalid event '{input}'"))?;

    let metric_suffix = match rename {
        Some(r) => r,
        None => &resolved.name,
    };

    Ok(ParsedEvent {
        metric_suffix: sanitize(metric_suffix),
        description: resolved.description,
        event: ConfiguredEvent {
            encoding: resolved.encoding,
            modifiers,
        },
    })
}

/// Resolve an event name (no modifiers) into a [`NamedPerfEvent`]. Each encoder builds the `NamedPerfEvent`
/// in its own module; this only dispatches to them. See the module docs for the recognised forms.
fn resolve_event(name: &str) -> anyhow::Result<NamedPerfEvent> {
    // raw-hex route (`rN`, hex register encoding): recognised, but not encoded yet.
    let looks_raw = name
        .strip_prefix('r')
        .map(|d| d.trim_start_matches("0x").trim_start_matches("0X"))
        .is_some_and(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_hexdigit()));
    if looks_raw {
        anyhow::bail!("raw-hex events (`{name}`) are not supported yet; this is planned for a future release");
    }
    // pmu-named (`pmu/event=,umask=/`) and pmu-raw (`pmu/config=,config1=/`) routes: recognised,
    // but not encoded yet.
    if name.contains('/') {
        anyhow::bail!(
            "pmu-named / pmu-raw events (`{name}`) are not supported yet; this is planned for a future release"
        );
    }

    // native route: try the built-in kernel tables first.
    if let Ok(e) = native::parse(name) {
        return Ok(e);
    }

    // Fall back to libpfm.
    pfm::encode(name)
        .with_context(|| format!("unknown event '{name}': not a native event, and libpfm could not encode it"))
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

    fn parse_simple(s: &str) -> ParsedEvent {
        parse(&EventEntry::Simple(s.to_owned())).unwrap()
    }

    #[test]
    fn native_hardware() {
        let e = parse_simple("REF_CPU_CYCLES");
        assert_eq!(e.metric_suffix, "ref_cpu_cycles");
        assert_eq!(e.event.encoding, EventEncoding::from_event(Hardware::REF_CPU_CYCLES));
    }

    #[test]
    fn native_software() {
        let e = parse_simple("CONTEXT_SWITCHES");
        assert_eq!(e.metric_suffix, "context_switches");
        assert_eq!(e.event.encoding, EventEncoding::from_event(Software::CONTEXT_SWITCHES));
    }

    #[test]
    fn native_cache() {
        let e = parse_simple("LL_READ_MISS");
        assert_eq!(e.metric_suffix, "ll_read_miss");
        assert_eq!(
            e.event.encoding,
            EventEncoding::from_event(Cache {
                which: CacheId::LL,
                operation: CacheOp::READ,
                result: CacheResult::MISS,
            })
        );
    }

    #[test]
    fn no_modifier_is_user_space_only() {
        // The default must match the original plugin: user space only (kernel + hv excluded).
        let e = parse_simple("INSTRUCTIONS");
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
        let x = parse_simple("INSTRUCTIONS#u").event.modifiers.excludes();
        assert!(!x.user && x.kernel && x.hv);
    }

    #[test]
    fn user_and_kernel_modifier() {
        // `#u:k` measures user and kernel, but still excludes the hypervisor.
        let x = parse_simple("INSTRUCTIONS#u:k").event.modifiers.excludes();
        assert!(!x.user);
        assert!(!x.kernel);
        assert!(x.hv);
    }

    #[test]
    fn modifiers_must_be_colon_separated() {
        // Modifiers are `:`-separated tokens; the grouped form `#uk` is rejected.
        assert!(parse(&EventEntry::Simple("INSTRUCTIONS#uk".to_owned())).is_err());
        let x = parse_simple("INSTRUCTIONS#u:k").event.modifiers.excludes();
        assert!(!x.user && !x.kernel && x.hv);
    }

    #[test]
    fn kernel_only_modifier() {
        // `#k` measures kernel only: user is excluded, kernel is counted.
        let x = parse_simple("INSTRUCTIONS#k").event.modifiers.excludes();
        assert!(x.user);
        assert!(!x.kernel);
        assert!(x.hv);
    }

    #[test]
    fn host_and_idle_modifiers() {
        let x = parse_simple("INSTRUCTIONS#H:I").event.modifiers.excludes();
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
        let e = parse_simple("INSTRUCTIONS#u");
        assert_eq!(e.metric_suffix, "instructions");
        assert_eq!(e.event.encoding, EventEncoding::from_event(Hardware::INSTRUCTIONS));
        assert!(!e.event.modifiers.excludes().user);
    }

    #[test]
    fn rename_overrides_metric_name() {
        let e = parse(&EventEntry::Detailed {
            event: "LL_READ_MISS".to_owned(),
            rename: Some("my llc miss".to_owned()),
        })
        .unwrap();
        assert_eq!(e.metric_suffix, "my_llc_miss");
    }

    #[test]
    fn raw_code_rejected_for_now() {
        // `rNNNN` is recognised by the syntax but not encoded yet.
        let err = parse(&EventEntry::Simple("r0x412e".to_owned())).unwrap_err();
        assert!(format!("{err:#}").contains("future release"), "got: {err:#}");
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
        // A generic name unknown to the native tables is resolved through libpfm, and produces the
        // same encoding as calling libpfm directly.
        let e = parse_simple("PERF_COUNT_HW_INSTRUCTIONS");
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
