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
    last_timestamp: LastMeasure,
}

#[derive(Default)]
struct LastMeasure {
    last_timestamp: Option<Timestamp>,
    last_power: Option<u64>,
}

impl LastMeasure {
    /// Compute an energy from a power. Using as time the time elapsed between
    /// self's timestamp and the current timestamp `timestamp`.
    ///
    /// This function first computes the time elapsed between two timestamps.
    /// It returns None if it's not possible.
    /// Finally it compute the energy using the formula: Energy(J) = ((Power_old(W) + Power_new(W)) / 2) * Time(s)
    ///
    /// Returns the computed energy on success or None for the first time
    pub fn compute_energy(&mut self, power: u64, timestamp: Timestamp) -> Option<f64> {
        if self.last_timestamp.is_none() {
            self.last_timestamp = Some(timestamp);
            self.last_power = Some(power);
            return None;
        }
        let time_elapsed = timestamp
            .duration_since(self.last_timestamp.unwrap())
            .expect("last timestamp should be before current_timestamp")
            .as_secs_f64();
        let energy_consumed = Some((self.last_power.unwrap() + power) as f64 / 2.0 * (time_elapsed));
        self.last_timestamp = Some(timestamp);
        self.last_power = Some(power);
        energy_consumed
    }
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
            last_timestamp: LastMeasure::default(),
        };
        Ok(probe)
    }
}

impl Source for GraceHopperProbe {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        let mut buffer = String::new();
        let power = read_power_value(&mut buffer, &mut self.file)?;
        if let Some(computed_energy) = self.last_timestamp.compute_energy(power, timestamp) {
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

#[cfg(test)]
mod tests {
    use alumet::measurement::Timestamp;
    use anyhow::Context;
    use std::fs::File;
    use std::io::Write;
    use std::time::Duration;
    use tempfile::tempdir;

    // use crate::probe::{compute_energy, read_power_value};
    use crate::probe::read_power_value;
    use crate::probe::LastMeasure;

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
        let mut lm_init = LastMeasure::default();
        let ts0 = Timestamp::now();
        // Only one measurement, can't compute energy -> power
        assert_eq!(None, lm_init.compute_energy(140, ts0));

        // lm_init.last_timestamp = Some(0 + Duration::from_secs(3));
        let ts6 = ts0 + Duration::from_secs(6);
        assert_eq!(495.0, lm_init.compute_energy(25, ts6).unwrap());

        lm_init.last_timestamp = Some(ts0 + Duration::from_secs(5));
        lm_init.last_power = Some(70);
        let ts55 = ts0 + Duration::from_millis(5500);
        assert_eq!(50.0, lm_init.compute_energy(130, ts55).unwrap());

        lm_init.last_timestamp = Some(lm_init.last_timestamp.unwrap() + Duration::from_millis(500));
        lm_init.last_power = Some(50);
        let ts10 = ts0 + Duration::from_secs(10);
        assert_eq!(250.0, lm_init.compute_energy(75, ts10).unwrap());

        lm_init.last_timestamp = Some(ts0 + Duration::from_secs(9));
        lm_init.last_power = Some(80);
        let ts97 = ts0 + Duration::from_millis(9700);
        assert_eq!(50.05, lm_init.compute_energy(63, ts97).unwrap());

        lm_init.last_timestamp = Some(ts0 + Duration::from_secs(15));
        lm_init.last_power = Some(70);
        let ts19 = ts0 + Duration::from_secs(19);
        assert_eq!(282.0, lm_init.compute_energy(71, ts19).unwrap());
    }
}
