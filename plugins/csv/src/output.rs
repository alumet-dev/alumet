use std::{
    collections::HashSet,
    fs::File,
    io::{self, BufWriter, Write},
    path::Path,
    time::SystemTime,
};

use alumet::measurement::WrappedMeasurementValue;
use alumet::{
    measurement::MeasurementBuffer,
    pipeline::elements::{error::WriteError, output::OutputContext},
};
use anyhow::Context;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::csv::CsvHelper;

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

impl alumet::pipeline::Output for CsvOutput {
    fn write(&mut self, measurements: &MeasurementBuffer, ctx: &OutputContext) -> Result<(), WriteError> {
        if self.attributes_in_header.is_none() && !measurements.is_empty() {
            // Collect the attributes that are present in the measurements.
            // Then, sort the keys to ensure a consistent order between calls to `CsvOutput::write`.
            let attr_keys = collect_attribute_keys(measurements);
            let mut attr_keys_sorted: Vec<&str> = attr_keys.iter().map(|s| s.as_str()).collect::<Vec<_>>();
            attr_keys_sorted.sort();

            // Build the CSV header
            let mut header = Vec::with_capacity(8 + attr_keys_sorted.len());
            header.extend(&[
                "metric",
                "timestamp",
                "value",
                "resource_kind",
                "resource_id",
                "consumer_kind",
                "consumer_id",
            ]);
            header.extend(attr_keys_sorted);
            header.push("__late_attributes");

            self.csv_helper.writeln(&mut self.writer, header)?;

            self.attributes_in_header = Some(attr_keys);
        }

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
            let datetime: String = datetime.format(&Rfc3339)?;
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

            // Sort the attributes by key
            let mut attr_sorted = m.attributes().collect::<Vec<_>>();
            attr_sorted.sort_by_key(|(k, _)| *k);

            // Handle known as well as new attributes.
            let mut late_attrs: String = String::new();
            let mut known_attrs = 0;
            for (key, value) in attr_sorted {
                if self.attributes_in_header.as_ref().unwrap().contains(key) {
                    // known attribute, write in the same order as the header (thanks to the sort)
                    record.push(value.to_string());
                    known_attrs += 1;
                } else {
                    // unknown attribute, add to the column `__late_attributes`
                    use std::fmt::Write;

                    if !late_attrs.is_empty() {
                        late_attrs.push_str(", ");
                    }
                    write!(
                        late_attrs,
                        "{}={}",
                        escape_late_attribute(key),
                        escape_late_attribute(&value.to_string())
                    )?;
                }
            }
            // Add missing attributes as empty values
            let missing_attributes = self.attributes_in_header.as_ref().unwrap().len() - known_attrs;
            record.extend(vec![String::from(""); missing_attributes]);

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
