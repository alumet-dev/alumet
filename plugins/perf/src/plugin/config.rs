//! The plugin configuration, and how an entry becomes events and metric names.
use std::time::Duration;

use alumet::metrics::TypedMetricId;
use serde::{Deserialize, Serialize};

use crate::event;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Config {
    #[serde(with = "humantime_serde")]
    pub(super) poll_interval: Duration,
    #[serde(with = "humantime_serde")]
    pub(super) flush_interval: Duration,

    /// The events to measure, described with the unified syntax (see [`event`]).
    ///
    /// Each entry is either a bare string (`"REF_CPU_CYCLES"`, `"INSTRUCTIONS#u"`) or an inline
    /// table with an optional metric `rename` (`{ event = "LL_READ_MISS", rename = "llc_miss" }`).
    pub(super) events: Vec<EventEntry>,

    /// If `true`, the perf sources will be started in pause state.
    /// The default value is `false`.
    ///
    /// This behavior is necessary to have fine-grained control over which source to monitor.
    /// !! It's essentially needed for advanced Alumet setup with a control plugin that manage the state of sources.
    #[serde(default)]
    pub(super) add_source_in_pause_state: bool,

    /// Whether to compensate for the multiplexing of the perf events.
    ///
    /// A CPU only has a few hardware counters. When more events are requested than it can hold, the
    /// kernel only counts them part of the time, and the raw values are underestimated. When this is
    /// enabled (the default), the plugin extrapolates the missing part, like the `perf` tool does.
    /// When disabled, the raw values are reported as they are.
    ///
    /// Either way, every measurement carries an `accuracy` attribute telling whether its value is
    /// exact, extrapolated or underestimated.
    pub(super) multiplexing_auto_scale: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(1), // 1Hz
            flush_interval: Duration::from_secs(5),

            events: vec![
                EventEntry::Simple("REF_CPU_CYCLES".to_owned()),
                EventEntry::Simple("CACHE_MISSES".to_owned()),
                EventEntry::Simple("BRANCH_MISSES".to_owned()),
                EventEntry::Simple("LL_READ_MISS".to_owned()),
            ],

            add_source_in_pause_state: false,

            multiplexing_auto_scale: true,
        }
    }
}

/// One entry of the `events` config list: a bare string, or a table with a metric `rename`.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub(super) enum EventEntry {
    Simple(String),
    Detailed {
        event: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rename: Option<String>,
    },
}

impl EventEntry {
    /// Parse the event; a `rename` replaces the name of every event it expands to, so that they all
    /// share the renamed metric.
    pub(super) fn parse(&self) -> anyhow::Result<Vec<event::ParsedEvent>> {
        match self {
            EventEntry::Simple(event) => event::parse(event),
            EventEntry::Detailed { event, rename } => {
                let mut events = event::parse(event)?;
                if let Some(rename) = rename {
                    for e in &mut events {
                        e.set_name(rename.clone());
                    }
                }
                Ok(events)
            }
        }
    }
}

// TODO proper deserialization with serde?
pub(super) struct ParsedConfig {
    pub(super) poll_interval: Duration,
    pub(super) flush_interval: Duration,

    pub(super) events: Vec<event::ParsedEvent>,
    pub(super) metrics: Vec<TypedMetricId<u64>>,

    pub(super) add_source_in_pause_state: bool,

    pub(super) multiplexing_auto_scale: bool,
}

/// Turn a string into a metric-name-safe suffix: letters are lowercased, non-alphanumeric
/// characters become `_`, and leading/trailing `_` are trimmed.
pub(super) fn sanitize(s: &str) -> String {
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
    use super::*;

    #[test]
    fn rename_overrides_metric_name() {
        let entry = EventEntry::Detailed {
            event: "LL_READ_MISS".to_owned(),
            rename: Some("my llc miss".to_owned()),
        };
        for parsed in &entry.parse().unwrap() {
            assert_eq!(sanitize(parsed.name()), "my_llc_miss");
        }
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
