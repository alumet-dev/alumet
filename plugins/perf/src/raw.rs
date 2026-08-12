//! Raw perf events, in the style of `perf stat -e`.
//!
//! One form is accepted:
//! - **`rN`** — a raw event code `N` (hexadecimal) on the default raw PMU (`PERF_TYPE_RAW`), e.g.
//!   `r3c` or `r0x412e`. `N` is written verbatim into `config`.

use perf_event_open_sys::bindings::PERF_TYPE_RAW;

use crate::spec::{EventEncoding, NamedPerfEvent, sanitize};

/// Try to parse `name` as a raw-hex event.
///
/// Returns `None` if `name` is not the raw form.
pub fn parse(name: &str) -> Option<anyhow::Result<NamedPerfEvent>> {
    let config = raw_config(name)?;
    Some(Ok(build(config, name)))
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

fn build(config: u64, original: &str) -> NamedPerfEvent {
    NamedPerfEvent {
        name: sanitize(original),
        description: format!("raw event {config:#x}"),
        encoding: EventEncoding {
            type_: PERF_TYPE_RAW,
            config,
            config1: 0,
            config2: 0,
        },
    }
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
        assert!(parse("cpu/config=0x3c/").is_none());
    }
}
