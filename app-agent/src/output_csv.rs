use std::{
    fs::File,
    io::{self, BufWriter, Write}, path::Path,
};

use alumet::metrics::{MetricId, WrappedMeasurementValue};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

pub struct CsvOutput {
    write_header: bool,
    force_flush: bool,
    writer: BufWriter<File>,
}

impl CsvOutput {
    pub fn new(output_file: impl AsRef<Path>, force_flush: bool) -> io::Result<Self> {
        let writer = BufWriter::new(File::create(output_file)?);
        Ok(Self { write_header: true, force_flush, writer })
    }
}

impl alumet::pipeline::Output for CsvOutput {
    fn write(&mut self, measurements: &alumet::metrics::MeasurementBuffer) -> Result<(), alumet::pipeline::WriteError> {
        if self.write_header {
            write!(self.writer, "metric;timestamp;value;resource_kind;resource_id\n")?;
            self.write_header = false;
        }
        for m in measurements.iter() {
            let metric_name = m.metric.name();
            let datetime: OffsetDateTime = m.timestamp.into();
            let datetime: String = datetime.format(&Rfc3339)?;
            let value = match m.value {
                WrappedMeasurementValue::F64(x) => x.to_string(),
                WrappedMeasurementValue::U64(x) => x.to_string(),
            };
            let resource_kind = m.resource.kind();
            let resource_id = m.resource.id_str();
            write!(self.writer, "{metric_name};{datetime};{value};{resource_kind};{resource_id}\n")?;
        }
        if self.force_flush {
            self.writer.flush()?;
        }
        Ok(())
    }
}
