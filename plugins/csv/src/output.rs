use std::{collections::HashSet, fs::File, path::Path, time::SystemTime};

use crate::csv::{CsvParams, CsvWriter};
use alumet::{
    measurement::MeasurementBuffer,
    pipeline::elements::{error::WriteError, output::OutputContext},
};
use alumet::{measurement::WrappedMeasurementValue, pipeline::Output};
use anyhow::Context;
use rustc_hash::FxHashMap;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

pub struct CsvOutput {
    /// parameter: do we flush after each write(measurements)?
    force_flush: bool,

    /// parameter: do we append the unit to the metric name?
    append_unit_to_metric_name: bool,
    use_unit_display_name: bool,

    /// CSV writer
    writer: CsvWriter,
}

pub struct CsvOutputSettings {
    pub force_flush: bool,
    pub append_unit_to_metric_name: bool,
    pub use_unit_display_name: bool,
    pub params: CsvParams,
}

impl CsvOutput {
    pub fn new(output_file: impl AsRef<Path>, settings: CsvOutputSettings) -> anyhow::Result<Self> {
        let path = output_file.as_ref();
        let file = File::create(path).with_context(|| format!("failed to open file for writing {path:?}"))?;
        let writer = CsvWriter::new(file, settings.params);
        Ok(Self {
            force_flush: settings.force_flush,
            append_unit_to_metric_name: settings.append_unit_to_metric_name,
            use_unit_display_name: settings.use_unit_display_name,
            writer,
        })
    }
}

fn collect_attribute_keys(buf: &MeasurementBuffer) -> HashSet<String> {
    let mut res = HashSet::new();
    for m in buf.iter() {
        res.extend(m.attributes_keys().map(|k| k.to_owned()));
    }
    res
}

impl Output for CsvOutput {
    fn write(&mut self, measurements: &MeasurementBuffer, ctx: &OutputContext) -> Result<(), WriteError> {
        log::trace!("writing csv measurements {measurements:?}");
        if !self.writer.is_initialized() {
            log::trace!("initializing csv header");
            // Collect the attributes that are present in the measurements.
            // Then, sort the keys to ensure a consistent order between calls to `CsvOutput::write`.
            let attr_keys: HashSet<String> = collect_attribute_keys(measurements).into_iter().collect();
            let mut attr_sorted: Vec<&str> = attr_keys.iter().map(|k| k.as_str()).collect();
            attr_sorted.sort();

            let mut header = Vec::with_capacity(8 + attr_sorted.len());
            header.extend(&[
                "metric",
                "timestamp",
                "value",
                "resource_kind",
                "resource_id",
                "consumer_kind",
                "consumer_id",
            ]);
            header.extend(attr_sorted);
            let header = header.into_iter().map(String::from).collect();
            log::trace!("writing header {header:?}");
            self.writer.write_header(header)?;
        }

        for m in measurements {
            log::trace!("writing {m:?}");
            let mut data = FxHashMap::default();

            let metric = ctx.metrics.by_id(&m.metric).expect("unknown metric");
            let metric_name = metric.name.clone();
            let unit = &metric.unit;

            let unit_string = if self.append_unit_to_metric_name {
                if self.use_unit_display_name {
                    unit.display_name()
                } else {
                    unit.unique_name()
                }
            } else {
                String::new()
            };
            let metric_string = if unit_string.is_empty() {
                metric_name
            } else {
                format!("{metric_name}_{unit_string}")
            };

            let datetime: OffsetDateTime = SystemTime::from(m.timestamp).into();
            let datetime = datetime.format(&Rfc3339)?;

            let value = match m.value {
                WrappedMeasurementValue::F64(x) => x.to_string(),
                WrappedMeasurementValue::U64(x) => x.to_string(),
            };
            let resource_kind = m.resource.kind().to_owned();
            let resource_id = m.resource.id_display().to_string();
            let consumer_kind = m.consumer.kind().to_owned();
            let consumer_id = m.consumer.id_display().to_string();

            data.insert("metric".to_owned(), metric_string);
            data.insert("timestamp".to_owned(), datetime);
            data.insert("value".to_owned(), value);
            data.insert("resource_kind".to_owned(), resource_kind);
            data.insert("resource_id".to_owned(), resource_id);
            data.insert("consumer_kind".to_owned(), consumer_kind);
            data.insert("consumer_id".to_owned(), consumer_id);

            for (k, v) in m.attributes() {
                data.insert(k.to_owned(), v.to_string());
            }

            self.writer.write_line(&mut data)?;
        }

        if self.force_flush {
            log::trace!("flushing BufWriter");
            self.writer.flush()?;
        }
        Ok(())
    }
}

fn escape_late_attribute(s: &str) -> String {
    s.replace('=', "\\=")
}

#[cfg(test)]
mod tests {

    use alumet::{
        measurement::{MeasurementBuffer, MeasurementPoint, Timestamp, WrappedMeasurementValue},
        metrics::RawMetricId,
        resources::{Resource, ResourceConsumer},
    };
    use std::{collections::HashSet, time::UNIX_EPOCH};

    use super::{collect_attribute_keys, escape_late_attribute};

    fn simple_point(metric: RawMetricId, value: WrappedMeasurementValue) -> MeasurementPoint {
        MeasurementPoint::new_untyped(
            Timestamp::from(UNIX_EPOCH),
            metric,
            Resource::LocalMachine,
            ResourceConsumer::LocalMachine,
            value,
        )
    }

    #[test]
    fn escape_late_attribute_string_with_equals() {
        let expected = String::from(" \\= a string \\= with \\= ");
        let result = escape_late_attribute(" = a string = with = ");
        assert_eq!(expected, result);
    }

    #[test]
    fn collect_attribute_keys_no_attributes() {
        let metric = RawMetricId::from_u64(0);
        let point = simple_point(metric, WrappedMeasurementValue::U64(0));
        let buf = MeasurementBuffer::from_iter([point]);

        let result = collect_attribute_keys(&buf);
        let expected = HashSet::new();
        assert_eq!(result, expected)
    }

    #[test]
    fn collect_attribute_keys_some_attributes() {
        let metric = RawMetricId::from_u64(0);
        let point = simple_point(metric, WrappedMeasurementValue::U64(0))
            .with_attr("k1", 123)
            .with_attr("k2", 456);
        let buf = MeasurementBuffer::from_iter([point]);

        let result = collect_attribute_keys(&buf);
        let expected = HashSet::from_iter(["k1".to_string(), "k2".to_string()]);
        assert_eq!(result, expected)
    }
}
