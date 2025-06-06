use std::{
    fs::File,
    io::{Read, Seek},
    time::Duration,
};

use anyhow::anyhow;

use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint, Timestamp},
    metrics::TypedMetricId,
    pipeline::{elements::error::PollError, Source},
    plugin::AlumetPluginStart,
    resources::{Resource, ResourceConsumer},
    units::{PrefixedUnit, Unit},
};

use crate::Sensor;

pub struct GraceHopperProbe {
    socket: u32,
    kind: String,
    file: File,
    consumer: ResourceConsumer,
    metric: Option<TypedMetricId<u64>>,
    _power_stats_interval: Duration,
}

impl GraceHopperProbe {
    pub fn new(alumet: &mut AlumetPluginStart, sensor: Sensor) -> Result<Self, anyhow::Error> {
        let metric = alumet
            .create_metric::<u64>(
                "consumption",
                PrefixedUnit::micro(Unit::Watt),
                "Power consumption of the sensor",
            )
            .ok();

        if !sensor.file.exists() {
            return Err(anyhow!("can't find the file: {:?} so no probe created", sensor.file));
        };
        let file = File::open(
            sensor
                .file
                .parent()
                .expect("power1_average file should exist")
                .join("power1_average"),
        )?;
        let probe: GraceHopperProbe = GraceHopperProbe {
            socket: sensor.socket,
            kind: sensor.kind.to_lowercase(),
            file,
            metric,
            consumer: ResourceConsumer::LocalMachine,
            _power_stats_interval: sensor.average_interval,
        };
        Ok(probe)
    }
}

impl Source for GraceHopperProbe {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        let mut buffer = String::new();
        let power = read_power_value(&mut buffer, &mut self.file)?;
        measurements.push(
            MeasurementPoint::new(
                timestamp,
                self.metric
                    .expect("can't push to the MeasurementAccumulator because can't retrieve the metric"),
                Resource::CpuPackage { id: self.socket },
                self.consumer.clone(),
                power,
            )
            .with_attr("sensor", self.kind.clone()),
        );

        Ok(())
    }
}

/// Reads and returns a power consumption value from a file.
///
/// This function clears the provided `buffer`, rewinds the `file` to the beginning,
/// reads its entire content into the buffer, and attempts to parse it as an
/// unsigned 64-bit integer (`u64`).
///
/// Returns the power consumption value on success
pub fn read_power_value(buffer: &mut String, file: &mut File) -> Result<u64, anyhow::Error> {
    buffer.clear();
    file.rewind()?;
    file.read_to_string(buffer)?;

    let power_consumption = match buffer.trim().parse::<u64>() {
        Ok(value) => value,
        Err(_) => {
            log::error!("can't parse the content of file {:?}, read: {:?}", file, buffer);
            0
        }
    };
    Ok(power_consumption)
}

#[cfg(test)]
mod tests {
    use anyhow::Context;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    use crate::probe::read_power_value;

    #[test]
    fn test_read_power_value() {
        let test_cases = vec![
            ("123456789", 123456789),
            ("585865", 585865),
            ("987123", 987123),
            ("5588", 5588),
            ("0", 0),
        ];

        for (line, expected_sensor) in test_cases {
            let root = tempdir().unwrap();
            let file_path = root.path().join("power1_oem");
            let mut file = File::create(&file_path).unwrap();
            writeln!(file, "{}", line).unwrap();
            let mut file = File::open(&file_path)
                .context("Failed to open the file")
                .expect("Can't open the file when testing read_power_value function");
            let mut buffer = String::new();
            let result = read_power_value(&mut buffer, &mut file);
            assert!(result.is_ok(), "Expected Ok for input '{}'", line);
            let power = result.unwrap();
            // Check content
            assert_eq!(power, expected_sensor, "Incorrect sensor for input '{}'", line);
        }
    }
}
