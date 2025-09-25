use std::{
    collections::{HashMap, HashSet},
    fmt::Write as OtherWrite,
    fs::File,
    io::{self, BufWriter, Write},
    path::Path,
    time::SystemTime,
};

use crate::csv::CsvHelper;
use alumet::{
    measurement::MeasurementBuffer,
    pipeline::elements::{error::WriteError, output::OutputContext},
};
use alumet::{measurement::WrappedMeasurementValue, pipeline::Output};
use anyhow::Context;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

pub struct CsvOutput {
    /// The attributes that we have written to the header.
    /// None if the header has not been written yet.
    attributes_in_header: Option<HashSet<String>>,

    /// parameter: do we flush after each write(measurements)?
    force_flush: bool,

    /// parameter: do we append the unit to the metric name?
    append_unit_to_metric_name: bool,
    use_unit_display_name: bool,

    /// File writer
    writer: BufWriter<File>,

    /// CSV utility
    csv_helper: CsvHelper,
}

impl CsvOutput {
    pub fn new(
        output_file: impl AsRef<Path>,
        force_flush: bool,
        append_unit_to_metric_name: bool,
        use_unit_display_name: bool,
        delimiter: char,
        escaped_quote: String,
    ) -> io::Result<Self> {
        let writer = BufWriter::new(File::create(output_file)?);
        let helper = CsvHelper::new(delimiter, escaped_quote);
        Ok(Self {
            attributes_in_header: None,
            force_flush,
            append_unit_to_metric_name,
            use_unit_display_name,
            writer,
            csv_helper: helper,
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
        if self.attributes_in_header.is_none() && !measurements.is_empty() {
            // Collect the attributes that are present in the measurements.
            // Then, sort the keys to ensure a consistent order between calls to `CsvOutput::write`.
            let attr_keys: HashSet<String> = collect_attribute_keys(measurements).into_iter().collect();
            let mut attr_sorted: Vec<String> = attr_keys.iter().cloned().collect();
            attr_sorted.sort();

            // Build the CSV header
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
            header.extend(attr_sorted.iter().map(String::as_str));
            header.push("__late_attributes");

            self.csv_helper.writeln(&mut self.writer, header)?;
            self.attributes_in_header = Some(attr_keys);
        }

        let attr_keys = self.attributes_in_header.as_ref().unwrap();
        let mut attr_sorted: Vec<&String> = attr_keys.iter().collect();
        attr_sorted.sort();

        for m in measurements.iter() {
            // get the full definition of the metric
            let full_metric = ctx
                .metrics
                .by_id(&m.metric)
                .with_context(|| format!("Unknown metric {:?}", m.metric))?;

            // extract the metric name, appending its unit if configured so
            let metric_name = if self.append_unit_to_metric_name {
                let unit_string = if self.use_unit_display_name {
                    full_metric.unit.display_name()
                } else {
                    full_metric.unit.unique_name()
                };
                if unit_string.is_empty() {
                    full_metric.name.to_owned()
                } else {
                    format!("{}_{}", full_metric.name, unit_string)
                }
            } else {
                full_metric.name.clone()
            };

            // convert every field to string
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

            // Start to build the record
            let mut record = vec![
                metric_name,
                datetime,
                value,
                resource_kind,
                resource_id,
                consumer_kind,
                consumer_id,
            ];

            // Handle known as well as new attributes.
            let mut known_attrs = HashMap::new();
            let mut late_attrs = String::new();

            for (key, value) in m.attributes() {
                let value_str = value.to_string();
                if attr_keys.contains(key) {
                    known_attrs.insert(key, value_str);
                } else {
                    if !late_attrs.is_empty() {
                        late_attrs.push_str(", ");
                    }
                    write!(
                        late_attrs,
                        "{}={}",
                        escape_late_attribute(key),
                        escape_late_attribute(&value_str)
                    )?;
                }
            }

            // Add attributes knows in order
            for key in &attr_sorted {
                record.push(known_attrs.get(key.as_str()).cloned().unwrap_or_default());
            }

            // Push the late attributes as one value
            record.push(late_attrs);
            // Write the record
            self.csv_helper.writeln(&mut self.writer, record)?;
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
