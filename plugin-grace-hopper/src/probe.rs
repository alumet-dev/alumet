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
    units::Unit,
};

use crate::Sensor;

pub struct GraceHopperProbe {
    socket: u32,
    kind: String,
    file: File,
    consumer: ResourceConsumer,
    metric: Option<TypedMetricId<f64>>,
    _power_stats_interval: Duration,
    last_timestamp: Option<Timestamp>,
}

impl GraceHopperProbe {
    pub fn new(alumet: &mut AlumetPluginStart, sensor: Sensor) -> Result<Self, anyhow::Error> {
        let metric = alumet
            .create_metric::<f64>("energy_consumed", Unit::Joule, "Energy consumption of the sensor")
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
            _power_stats_interval: sensor._average_interval,
            last_timestamp: None,
        };
        Ok(probe)
    }
}

impl Source for GraceHopperProbe {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        let mut buffer = String::new();
        let power = read_power_value(&mut buffer, &mut self.file)?;
        if let Some(computed_energy) = compute_energy(power, &mut self.last_timestamp, timestamp) {
            measurements.push(
                MeasurementPoint::new(
                    timestamp,
                    self.metric
                        .expect("can't push to the MeasurementAccumulator because can't retrieve the metric"),
                    Resource::CpuPackage { id: self.socket },
                    self.consumer.clone(),
                    computed_energy,
                )
                .with_attr("sensor", self.kind.clone()),
            );
        }
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

/// Compute an energy from a power. Using as time the time elapsed between
/// `last_timestamp` and the current timestamp `timestamp`.
///
/// This function first computes as `(u64, u32)` the current Timestamp and last_timestamp stored.
/// Then it computes the time elapsed between the two timestamps.
/// Finally it compute the energy using the formula: Energy(J) = Power(W) * Time(s)
///
/// Returns the computed energy on success or None for the first time
pub fn compute_energy(power: u64, last_timestamp: &mut Option<Timestamp>, timestamp: Timestamp) -> Option<f64> {
    if last_timestamp.is_none() {
        *last_timestamp = Some(timestamp);
        return None;
    }
    let time_elapsed = timestamp
        .duration_since(last_timestamp.unwrap())
        .expect("last timestamp should be before current_timestamp")
        .as_secs_f64();
    *last_timestamp = Some(timestamp);
    Some(power as f64 * time_elapsed)
}

#[cfg(test)]
mod tests {
    use alumet::measurement::Timestamp;
    use anyhow::Context;
    use std::fs::File;
    use std::io::Write;
    use std::time::Duration;
    use tempfile::tempdir;

    use crate::probe::{compute_energy, read_power_value};

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

    #[test]
    fn test_compute_energy() {
        // let mut c1 = CounterDiff::with_max_value(u64::MAX);
        let ts1 = Timestamp::now();
        let power1 = 1;
        let power2 = 2;
        // Only one measurement, can't compute energy -> power
        assert_eq!(None, compute_energy(power1, &mut None, ts1));

        let ts2 = ts1 + Duration::from_secs(1);
        // 1s at 2W -> 2J
        assert_eq!(2.0, compute_energy(power2, &mut Some(ts1), ts2).unwrap());

        // Create a timestamp at 130s after the previous CounterDiff value (ts2), at 130W -> 130J
        let ts3 = ts1 + Duration::from_secs(131);
        assert_eq!(130.0, compute_energy(power1, &mut Some(ts2), ts3).unwrap());

        let power3 = 75;
        let ts4 = ts3 + Duration::from_secs(3);
        assert_eq!(225.0, compute_energy(power3, &mut Some(ts3), ts4).unwrap());
    }
}
