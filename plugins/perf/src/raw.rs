//! Raw perf events, in the style of `perf stat -e`.
//!
//! Two forms are accepted:
//! - **`rN`**: a raw event code `N` (hexadecimal) on the default raw PMU (`PERF_TYPE_RAW`), e.g.
//!   `r3c` or `r0x412e`. `N` is written verbatim into `config`.
//! - **`pmu/rN`**: the same raw code, but on a specific PMU whose numeric `type` is read from sysfs,
//!   e.g. `uncore_imc_0/r0x1`. This is the only way to reach a PMU (uncore, `power`, …) that has no
//!   generic namespace.

use perf_event_open_sys::bindings::PERF_TYPE_RAW;

use crate::pmu;
use crate::spec::{EventEncoding, NamedPerfEvent, sanitize};

/// Try to parse `name` as a raw-hex event.
///
/// Returns `None` if `name` is neither `rN` nor `pmu/rN`.
/// Returns `Some(Err(_))` when the form matched but the PMU `type` could not be read.
pub fn parse(name: &str) -> Option<anyhow::Result<NamedPerfEvent>> {
    // `pmu/rN`: a raw code on a specific PMU.
    if let Some((pmu, term)) = pmu::split(name) {
        let config = raw_config(term)?;
        return Some(build(Some(pmu), config, name));
    }
    // `rN`: a raw code on the default raw PMU.
    let config = raw_config(name)?;
    Some(build(None, config, name))
}

/// Parse a raw code token `r<hex>` (with an optional `0x` prefix) into its `config` value.
/// e.g. `r3c` -> `0x3c`, `r0x412e` -> `0x412e`. Returns `None` if `token` is not that shape.
fn raw_config(token: &str) -> Option<u64> {
    let digits = token.strip_prefix('r')?;
    let digits = digits.trim_start_matches("0x").trim_start_matches("0X");
    if digits.is_empty() {
        return None;
    }
    u64::from_str_radix(digits, 16).ok()
}

fn build(pmu: Option<&str>, config: u64, original: &str) -> anyhow::Result<NamedPerfEvent> {
    let (type_, description) = match pmu {
        Some(pmu) => (pmu::read_type(pmu)?, format!("raw event {config:#x} on PMU {pmu}")),
        None => (PERF_TYPE_RAW, format!("raw event {config:#x}")),
    };
    Ok(NamedPerfEvent {
        name: sanitize(original),
        description,
        encoding: EventEncoding {
            type_,
            config,
            config1: 0,
            config2: 0,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_config_parses_hex() {
        assert_eq!(raw_config("r3c"), Some(0x3c));
        assert_eq!(raw_config("r0x412e"), Some(0x412e));
        assert_eq!(raw_config("r0X412E"), Some(0x412e));
        assert_eq!(raw_config("r0"), Some(0));
    }

    #[test]
    fn raw_config_rejects_non_raw() {
        assert_eq!(raw_config("INSTRUCTIONS"), None); // not r-prefixed
        assert_eq!(raw_config("r"), None); // no digits
        assert_eq!(raw_config("r0x"), None); // no digits after prefix
        assert_eq!(raw_config("rZZ"), None); // not hex
        assert_eq!(raw_config("r3c_extra"), None); // trailing junk
    }

    #[test]
    fn plain_raw_encodes_to_perf_type_raw() {
        let e = parse("r0x412e").expect("recognised as raw").expect("valid");
        assert_eq!(e.name, "r0x412e");
        assert_eq!(
            e.encoding,
            EventEncoding {
                type_: PERF_TYPE_RAW,
                config: 0x412e,
                config1: 0,
                config2: 0,
            }
        );
    }

    #[test]
    fn non_raw_name_is_not_recognised() {
        assert!(parse("INSTRUCTIONS").is_none());
        assert!(parse("cpu/config=0x3c/").is_none()); // pmu-named term, not `rN`
        assert!(parse("cpu_core/INSTRUCTIONS").is_none()); // native-on-pmu, not `rN`
    }

    #[test]
    fn pmu_raw_uses_the_pmu_type() {
        // `pmu/rN` encodes on that PMU's numeric `type` (read from sysfs). Uses a real PMU so the
        // encoding reflects it; skips cleanly if /sys is unavailable (e.g. a sandbox).
        let Some((pmu, expected_type)) = first_available_pmu() else {
            eprintln!("skipping pmu_raw_uses_the_pmu_type: no PMU found in sysfs");
            return;
        };
        let name = format!("{pmu}/r0x1/");
        let e = parse(&name).expect("recognised as raw").expect("valid");
        assert_eq!(e.encoding.type_, expected_type);
        assert_eq!(e.encoding.config, 0x1);
        assert_eq!(e.name, sanitize(&name));
    }

    #[test]
    fn pmu_raw_with_unknown_pmu_errors() {
        // The `pmu/rN` shape matched, but the PMU does not exist: this must surface as an error,
        // not as "not recognised".
        let result = parse("definitely_not_a_pmu_xyz/r0x1").expect("recognised as pmu/rN");
        assert!(result.is_err());
    }

    /// Find any PMU exposing a numeric `type` in sysfs, returning its name and type.
    fn first_available_pmu() -> Option<(String, u32)> {
        let entries = std::fs::read_dir("/sys/bus/event_source/devices").ok()?;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Ok(t) = pmu::read_type(&name) {
                return Some((name, t));
            }
        }
        None
    }
}
