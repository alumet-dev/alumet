//! The pipeline-facing [`Output`]. On every flush it merges the measurements into the shared
//! [`Model`] and evicts stale series. The TUI thread owns the terminal and redraws from the model;
//! this output never touches the terminal itself.

use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime};

use alumet::{
    measurement::{MeasurementBuffer, MeasurementPoint, WrappedMeasurementValue},
    pipeline::{
        Output,
        elements::{error::WriteError, output::OutputContext},
    },
};
use time::OffsetDateTime;
use time::macros::format_description;

use crate::model::{Model, SeriesKey, SeriesValue};

/// Format used for the `updated` column (`HH:MM:SS`).
const TIME_FORMAT: &[time::format_description::FormatItem] = format_description!("[hour]:[minute]:[second]");

pub struct TuiOutputSettings {
    pub print_unit: bool,
    pub use_unit_display_name: bool,
}

pub struct TuiOutput {
    shared: Arc<Mutex<Model>>,
    settings: TuiOutputSettings,
}

impl TuiOutput {
    pub fn new(shared: Arc<Mutex<Model>>, settings: TuiOutputSettings) -> Self {
        Self { shared, settings }
    }

    /// Extracts the series identity and latest value from a single measurement.
    fn series_of(&self, m: &MeasurementPoint, ctx: &OutputContext, now: Instant) -> (SeriesKey, SeriesValue) {
        let (metric_name, unit) = match ctx.metrics.by_id(&m.metric) {
            Some(metric) => {
                let unit = if self.settings.use_unit_display_name {
                    metric.unit.display_name()
                } else {
                    metric.unit.unique_name()
                };
                (metric.name.clone(), unit)
            }
            None => (format!("<unknown metric {:?}>", m.metric), String::new()),
        };

        let value_num = m.value.as_f64();
        let value = match m.value {
            WrappedMeasurementValue::F64(x) => format!("{x}"),
            WrappedMeasurementValue::U64(x) => x.to_string(),
        };

        let updated = OffsetDateTime::from(SystemTime::from(m.timestamp))
            .format(TIME_FORMAT)
            .unwrap_or_else(|_| String::from("?"));

        let mut attributes: Vec<String> = m.attributes().map(|(k, v)| format!("{k}={v}")).collect();
        attributes.sort();

        let key = SeriesKey {
            metric: metric_name,
            unit: if self.settings.print_unit { unit } else { String::new() },
            resource: display_resource(m.resource.kind(), &m.resource.id_display().to_string()),
            consumer: display_resource(m.consumer.kind(), &m.consumer.id_display().to_string()),
            attributes: attributes.join(", "),
        };
        let value = SeriesValue {
            value,
            value_num,
            updated,
            last_seen: now,
        };
        (key, value)
    }
}

impl Output for TuiOutput {
    fn write(&mut self, measurements: &MeasurementBuffer, ctx: &OutputContext) -> Result<(), WriteError> {
        if measurements.is_empty() {
            return Ok(());
        }

        let now = Instant::now();

        // Merge into the shared model and drop stale series. The TUI thread does the drawing.
        let mut model = self.shared.lock().expect("model mutex poisoned");
        for m in measurements.iter() {
            let (key, value) = self.series_of(m, ctx, now);
            model.upsert(key, value);
        }
        model.evict(now);
        Ok(())
    }
}

/// Formats a resource/consumer as `kind(id)`, or just `kind` when there is no id.
fn display_resource(kind: &str, id: &str) -> String {
    if id.is_empty() {
        kind.to_owned()
    } else {
        format!("{kind}({id})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn display_resource_formats_id() {
        assert_eq!(display_resource("cpu_package", "0"), "cpu_package(0)");
        assert_eq!(display_resource("local_machine", ""), "local_machine");
    }
}
