use std::{
    collections::HashSet,
    fs::File,
    io::{self, BufWriter, Write},
    path::Path, time::SystemTime,
};

use alumet::{measurement::WrappedMeasurementValue, pipeline::OutputContext};
use alumet::{measurement::MeasurementBuffer, metrics::MetricId};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

pub struct CsvOutput {
    /// The attributes that we have written to the header.
    /// None if the header has not been written yet.
    attributes_in_header: Option<HashSet<String>>,

    /// parameter: do we flush after each write(measurements) ?
    force_flush: bool,
    /// writer
    writer: BufWriter<File>,
}

impl CsvOutput {
    pub fn new(output_file: impl AsRef<Path>, force_flush: bool) -> io::Result<Self> {
        let writer = BufWriter::new(File::create(output_file)?);
        Ok(Self {
            attributes_in_header: None,
            force_flush,
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

impl alumet::pipeline::Output for CsvOutput {
    fn write(&mut self, measurements: &MeasurementBuffer, ctx: &OutputContext) -> Result<(), alumet::pipeline::WriteError> {
        if self.attributes_in_header.is_none() && measurements.len() > 0 {
            let attr_keys = collect_attribute_keys(measurements);

            // sort the keys to ensure a consistent order between calls to write(measurements)
            let mut attr_keys_sorted: Vec<&str> = attr_keys.iter().map(|s| s.as_str()).collect::<Vec<_>>();
            attr_keys_sorted.sort();

            let attributes_in_header = attr_keys_sorted.join(";");
            let sep = if attributes_in_header.is_empty() { "" } else { ";" };
            write!(
                self.writer,
                "metric;timestamp;value;resource_kind;resource_id;consumer_kind;consumer_id{sep}{attributes_in_header};__late_attributes\n"
            )?;

            self.attributes_in_header = Some(attr_keys);
        }

        for m in measurements.iter() {
            let metric_name = m.metric.name(ctx);
            let datetime: OffsetDateTime = SystemTime::from(m.timestamp).into();
            let datetime: String = datetime.format(&Rfc3339)?;
            let value = match m.value {
                WrappedMeasurementValue::F64(x) => x.to_string(),
                WrappedMeasurementValue::U64(x) => x.to_string(),
            };
            let resource_kind = m.resource.kind();
            let resource_id = m.resource.id_display();
            let consumer_kind = m.consumer.kind();
            let consumer_id = m.consumer.id_display();
            write!(
                self.writer,
                "{metric_name};{datetime};{value};{resource_kind};{resource_id};{consumer_kind};{consumer_id}"
            )?;

            // sort the attributes by key
            let mut attr_sorted = m.attributes().collect::<Vec<_>>();
            attr_sorted.sort_by_key(|(k, _)| *k);

            let mut late_attrs = Vec::new();
            for (key, value) in attr_sorted {
                if self.attributes_in_header.as_ref().unwrap().contains(key) {
                    // known attribute, write in the same order as the header (thanks to the sort)
                    write!(self.writer, ";{value}")?;
                    // TODO escape value strings
                } else {
                    // unknown attribute, add to the column __late_attributes
                    late_attrs.push((key, value));
                }
            }
            write!(self.writer, ";")?;

            // write __late_attributes
            let mut first = false;
            for (key, value) in late_attrs {
                if first {
                    first = false;
                } else {
                    write!(self.writer, ",")?;
                }
                write!(self.writer, "{key}={value}")?;
            }

            // end of the record
            write!(self.writer, "\n")?;
        }
        if self.force_flush {
            log::trace!("flushing BufWriter");
            self.writer.flush()?;
        }
        Ok(())
    }
}
