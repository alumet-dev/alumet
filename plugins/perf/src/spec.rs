//! Unified perf event descriptor.
//!
//! An event is written `<event>[:<modifiers>]`, following the `perf stat -e` syntax. A single
//! parser owns the syntax: it splits off the modifiers (`:u`, `:k`, …) and applies them itself,
//! so a modifier behaves identically whatever the event's source.
//!
//! `<event>` can take several forms. We support a subset for now and *recognise* the rest so the
//! syntax stays stable across releases:
//!
//! - a **symbolic event name**, e.g. `INSTRUCTIONS` or `LL_READ_MISS` **supported**, encoded from
//!   the native kernel tables (hardware/software/cache). Encoding arbitrary names through libpfm is
//!   planned.
//! - a **raw PMU event** `rN`, where `N` is a hexadecimal value representing the raw register
//!   encoding, with the layout described by `/sys/bus/event_source/devices/cpu/format/*`
//!   **not yet supported** (planned).
//! - a **symbolically formed PMU event** `pmu/config=M,config1=N,config2=K/`, where `M`, `N`, `K`
//!   are numbers whose acceptable values are defined by
//!   `/sys/bus/event_source/devices/<pmu>/format/*` **not yet supported** (planned).
//! - the named-parameter variant `pmu/param1=0x3,param2/` and the uncore/`percore` qualifiers
//!   **not yet supported** (planned).
//!
//! A not-yet-supported form is recognised by [`resolve_event`] and rejected with an explicit "planned for a
//! future release" error.

use anyhow::Context;
use perf_event::events::{Cache, Event, Hardware, Software};
use perf_event_open_sys::bindings::perf_event_attr;
use serde::{Deserialize, Serialize};

use crate::events;

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

/// perf event domain modifiers, e.g. `INSTRUCTIONS:u:k`.
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
    /// Parse the modifier characters that follow the first `:` (the group separators `:` in
    /// `u:k` are ignored).
    fn parse(s: &str) -> anyhow::Result<Self> {
        let mut m = Modifiers::default();
        for c in s.chars() {
            match c {
                ':' => {}
                'u' => m.user = true,
                'k' => m.kernel = true,
                'h' => m.hv = true,
                'H' => m.host = true,
                'G' => m.guest = true,
                'I' => m.idle_only = true,
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

/// An encoded event, whatever its source. Future sources (`Raw`, `Pfm`) are added as new variants.
#[derive(Debug, Clone)]
enum AnyEvent {
    Hardware(Hardware),
    Software(Software),
    Cache(Cache),
}

impl AnyEvent {
    fn update_attrs(self, attr: &mut perf_event_attr) {
        match self {
            AnyEvent::Hardware(e) => e.update_attrs(attr),
            AnyEvent::Software(e) => e.update_attrs(attr),
            AnyEvent::Cache(e) => e.update_attrs(attr),
        }
    }
}

/// A fully-configured event ready to be added to a perf group: an encoding plus its modifiers.
#[derive(Debug, Clone)]
pub struct ConfiguredEvent {
    inner: AnyEvent,
    modifiers: Modifiers,
}

impl Event for ConfiguredEvent {
    fn update_attrs(self, attr: &mut perf_event_attr) {
        // Only encode the event here. The modifiers are applied by [`Self::configure`] *after*
        // `Builder::new`, otherwise its forced `exclude_kernel`/`exclude_hv` defaults would clobber
        // them.
        self.inner.update_attrs(attr);
    }
}

impl ConfiguredEvent {
    /// Apply this event's modifiers to a freshly-created builder. Required for the modifiers to
    /// take effect.
    pub fn configure(&self, builder: &mut perf_event::Builder<'_>) {
        self.modifiers.configure(builder);
    }
}

/// A parsed config event: the metric name suffix (after `perf_`), a description and the event.
#[derive(Debug)]
pub struct ParsedEvent {
    pub metric_suffix: String,
    pub description: String,
    pub event: ConfiguredEvent,
}

/// Parse one config entry into a [`ParsedEvent`].
pub fn parse(entry: &EventEntry) -> anyhow::Result<ParsedEvent> {
    let (input, rename) = entry.parts();
    let (name, mods_str) = input.split_once(':').unwrap_or((input, ""));
    if name.is_empty() {
        anyhow::bail!("empty event name in '{input}'");
    }

    let ctx = || format!("invalid event '{input}'");
    let modifiers = Modifiers::parse(mods_str).with_context(ctx)?;
    let (event, canonical_name, description) = resolve_event(name).with_context(ctx)?;

    let metric_suffix = match rename {
        Some(r) => sanitize(r),
        None => canonical_name,
    };

    Ok(ParsedEvent {
        metric_suffix,
        description,
        event: ConfiguredEvent {
            inner: event,
            modifiers,
        },
    })
}

/// Resolve an event (no modifiers) to its encoding, returning the event, its canonical name (used
/// for the metric name) and a description. See the module docs for the recognised forms.
fn resolve_event(name: &str) -> anyhow::Result<(AnyEvent, String, String)> {
    // Forms we recognise but do not encode yet: reject them explicitly so the syntax stays stable
    // and the error is actionable.

    // Raw PMU event `rN` (hex register encoding).
    let looks_raw = name
        .strip_prefix('r')
        .map(|d| d.trim_start_matches("0x").trim_start_matches("0X"))
        .is_some_and(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_hexdigit()));
    if looks_raw {
        anyhow::bail!("raw PMU events (`{name}`) are not supported yet; this is planned for a future release");
    }
    // Symbolically formed PMU event `pmu/.../`.
    if name.contains('/') {
        anyhow::bail!("PMU-term events (`{name}`) are not supported yet; this is planned for a future release");
    }

    // Symbolic event name: encoded from the native kernel tables.
    if let Ok(e) = events::parse_hardware(name) {
        return Ok((AnyEvent::Hardware(e.event), e.name, e.description));
    }
    if let Ok(e) = events::parse_software(name) {
        return Ok((AnyEvent::Software(e.event), e.name, e.description));
    }
    if let Ok(e) = events::parse_cache(name) {
        return Ok((AnyEvent::Cache(e.event), e.name, e.description));
    }
    anyhow::bail!(
        "Unknown event '{name}': not a native hardware/software/cache event. \
         Encoding arbitrary event names through libpfm is planned for a future release."
    )
}

/// Turn a string into a metric-name-safe suffix: non-alphanumeric characters become `_`, and
/// leading/trailing `_` are trimmed.
fn sanitize(s: &str) -> String {
    let mapped: String = s
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    mapped.trim_matches('_').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_simple(s: &str) -> ParsedEvent {
        parse(&EventEntry::Simple(s.to_owned())).unwrap()
    }

    #[test]
    fn native_hardware() {
        let e = parse_simple("REF_CPU_CYCLES");
        assert_eq!(e.metric_suffix, "REF_CPU_CYCLES");
        assert!(matches!(e.event.inner, AnyEvent::Hardware(_)));
    }

    #[test]
    fn native_software() {
        let e = parse_simple("CONTEXT_SWITCHES");
        assert_eq!(e.metric_suffix, "CONTEXT_SWITCHES");
        assert!(matches!(e.event.inner, AnyEvent::Software(_)));
    }

    #[test]
    fn native_cache() {
        let e = parse_simple("LL_READ_MISS");
        assert_eq!(e.metric_suffix, "LL_READ_MISS");
        assert!(matches!(e.event.inner, AnyEvent::Cache(_)));
    }

    #[test]
    fn no_modifier_is_user_space_only() {
        // The default must match the original plugin: user space only (kernel + hv excluded).
        let e = parse_simple("INSTRUCTIONS");
        assert_eq!(e.metric_suffix, "INSTRUCTIONS");
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
        let x = parse_simple("INSTRUCTIONS:u").event.modifiers.excludes();
        assert!(!x.user && x.kernel && x.hv);
    }

    #[test]
    fn user_and_kernel_modifier() {
        // `:u:k` measures user and kernel, but still excludes the hypervisor.
        let x = parse_simple("INSTRUCTIONS:u:k").event.modifiers.excludes();
        assert!(!x.user);
        assert!(!x.kernel);
        assert!(x.hv);
    }

    #[test]
    fn grouped_and_chained_modifiers_are_equivalent() {
        // `:uk` (grouped) and `:u:k` (each after its own colon) must mean the same thing.
        let grouped = parse_simple("INSTRUCTIONS:uk").event.modifiers.excludes();
        let chained = parse_simple("INSTRUCTIONS:u:k").event.modifiers.excludes();
        assert_eq!(grouped, chained);
    }

    #[test]
    fn kernel_only_modifier() {
        // `:k` measures kernel only: user is excluded, kernel is counted.
        let x = parse_simple("INSTRUCTIONS:k").event.modifiers.excludes();
        assert!(x.user);
        assert!(!x.kernel);
        assert!(x.hv);
    }

    #[test]
    fn host_and_idle_modifiers() {
        let x = parse_simple("INSTRUCTIONS:H:I").event.modifiers.excludes();
        assert!(x.guest); // host only -> exclude guest
        assert!(!x.host);
        assert!(x.idle); // exclude idle
    }

    #[test]
    fn unknown_modifier_is_rejected() {
        assert!(parse(&EventEntry::Simple("INSTRUCTIONS:z".to_owned())).is_err());
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
        let err = parse(&EventEntry::Simple("RESOURCE_STALLS".to_owned())).unwrap_err();
        assert!(format!("{err:#}").contains("libpfm"), "got: {err:#}");
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
