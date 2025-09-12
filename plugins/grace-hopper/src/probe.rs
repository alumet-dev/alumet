use std::{
    fs::File,
    io::{Read, Seek},
};

use anyhow::anyhow;

use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint, Timestamp},
    metrics::TypedMetricId,
    pipeline::{Source, elements::error::PollError},
    plugin::AlumetPluginStart,
    resources::{Resource, ResourceConsumer},
    units::Unit,
};

use crate::Sensor;

// #[derive(Debug)]
pub struct Probe {
    /// Kind of probe, could be either: module, grace, cpu, sysio
    kind: String,
    /// Socket associated to the probe
    socket: u32,
    file: File,
    last_measure: Option<PowerMeasure>,
}

pub struct GraceHopperProbe {
    // socket: u32,
    // kind: String,
    // file: File,
    probes: Vec<Probe>,
    consumer: ResourceConsumer,
    metric: TypedMetricId<f64>,
}

struct PowerMeasure {
    timestamp: Timestamp,
    power: u64,
}

impl PowerMeasure {
    /// Compute an energy from a power of a `PowerMeasure`. Using as time the time elapsed between
    /// self's timestamp and the timestamp of `PowerMeasure`.
    ///
    /// This function first computes the time elapsed between two timestamps.
    /// It return an error if ot's not possible
    /// Finally it compute the energy using the formula: Energy(J) = ((Power_old(W) + Power_new(W)) / 2) * Time(s)
    ///
    /// Returns the computed energy
    pub fn compute_energy(&self, measure: &PowerMeasure) -> anyhow::Result<f64> {
        let time_elapsed = measure.timestamp.duration_since(self.timestamp)?.as_secs_f64();
        let energy_consumed = (((self.power + measure.power) / 1_000_000) as f64 / 2.0) * time_elapsed; // Divided by 10e6 because of µW
        Ok(energy_consumed)
    }
}

impl GraceHopperProbe {
    pub fn new(alumet: &mut AlumetPluginStart, sensors: Vec<Sensor>) -> anyhow::Result<Self> {
        let metric = alumet.create_metric::<f64>("energy_consumed", Unit::Joule, "Energy consumption of the sensor")?;
        let mut all_sensors = Vec::<Probe>::new();
        for sensor in sensors {
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
            all_sensors.push(Probe {
                kind: sensor.kind,
                socket: sensor.socket,
                file,
                last_measure: None,
            });
        }
        let probe: GraceHopperProbe = GraceHopperProbe {
            probes: all_sensors,
            metric,
            consumer: ResourceConsumer::LocalMachine,
        };
        Ok(probe)
    }
}

impl Source for GraceHopperProbe {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        let mut buffer = String::new();
        let mut module_total = 0;
        let mut grace_total = 0;
        let mut cpu_total = 0;
        let mut sysio_total = 0;
        for module in self.probes.iter_mut() {
            let power = read_power_value(&mut buffer, &mut module.file).map_err(PollError::from)?;
            let new_measure = PowerMeasure { timestamp, power };

            if module.kind == "module" {
                module_total += power;
            }
            if module.kind == "grace" {
                grace_total += power;
            }
            if module.kind == "cpu" {
                cpu_total += power;
            }
            if module.kind == "sysio" {
                sysio_total += power;
            }

            if let Some(last_measure) = &module.last_measure {
                let computed_energy = last_measure.compute_energy(&new_measure)?;
                measurements.push(
                    MeasurementPoint::new(
                        timestamp,
                        self.metric,
                        Resource::CpuPackage { id: module.socket },
                        self.consumer.clone(),
                        computed_energy,
                    )
                    .with_attr("sensor", module.kind.clone()),
                );
            }
            module.last_measure = Some(PowerMeasure {
                timestamp: new_measure.timestamp,
                power: new_measure.power,
            });
        }
        measurements.push(
            MeasurementPoint::new(
                timestamp,
                self.metric,
                Resource::LocalMachine,
                self.consumer.clone(),
                (module_total / 1_000_000) as f64,
            )
            .with_attr("sensor", "module"),
        );
        measurements.push(
            MeasurementPoint::new(
                timestamp,
                self.metric,
                Resource::LocalMachine,
                self.consumer.clone(),
                (grace_total / 1_000_000) as f64,
            )
            .with_attr("sensor", "grace"),
        );
        measurements.push(
            MeasurementPoint::new(
                timestamp,
                self.metric,
                Resource::LocalMachine,
                self.consumer.clone(),
                (cpu_total / 1_000_000) as f64,
            )
            .with_attr("sensor", "cpu"),
        );
        measurements.push(
            MeasurementPoint::new(
                timestamp,
                self.metric,
                Resource::LocalMachine,
                self.consumer.clone(),
                (sysio_total / 1_000_000) as f64,
            )
            .with_attr("sensor", "sysio"),
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
    use alumet::measurement::Timestamp;
    use anyhow::Context;
    use std::fs::File;
    use std::io::Write;
    use std::time::Duration;
    use tempfile::tempdir;

    // use crate::probe::{compute_energy, read_power_value};
    use crate::probe::PowerMeasure;
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

    #[test]
    fn test_compute_energy() {
        let ts0 = Timestamp::now();
        let mut lm_init = PowerMeasure {
            timestamp: ts0,
            power: 0,
        };
        // timestamp diff is 0, can't compute energy -> 0
        let mut measure = PowerMeasure {
            timestamp: ts0,
            power: 140_000000,
        };
        assert_eq!(0.0, lm_init.compute_energy(&measure).unwrap());
        lm_init.power = measure.power;

        let ts6 = ts0 + Duration::from_secs(6);
        measure = PowerMeasure {
            timestamp: ts6,
            power: 25_000000,
        };
        assert_eq!(495.0, lm_init.compute_energy(&measure).unwrap());
        lm_init.power = measure.power;
        lm_init.timestamp = measure.timestamp;

        lm_init.timestamp = ts0 + Duration::from_secs(5);
        lm_init.power = 70_000000;
        let ts55 = ts0 + Duration::from_millis(5500);
        measure = PowerMeasure {
            timestamp: ts55,
            power: 130_000000,
        };
        assert_eq!(50.0, lm_init.compute_energy(&measure).unwrap());
        lm_init.timestamp = measure.timestamp;

        lm_init.timestamp = lm_init.timestamp + Duration::from_millis(500);
        lm_init.power = 50_000000;
        let ts10 = ts0 + Duration::from_secs(10);
        measure = PowerMeasure {
            timestamp: ts10,
            power: 75_000000,
        };
        assert_eq!(250.0, lm_init.compute_energy(&measure).unwrap());

        lm_init.timestamp = ts0 + Duration::from_secs(9);
        lm_init.power = 80_000000;
        let ts97 = ts0 + Duration::from_millis(9700);
        measure = PowerMeasure {
            timestamp: ts97,
            power: 63_000000,
        };
        assert_eq!(50.05, lm_init.compute_energy(&measure).unwrap());

        lm_init.timestamp = ts0 + Duration::from_secs(15);
        lm_init.power = 70_000000;
        let ts19 = ts0 + Duration::from_secs(19);
        measure = PowerMeasure {
            timestamp: ts19,
            power: 71_000000,
        };
        assert_eq!(282.0, lm_init.compute_energy(&measure).unwrap());
    }
}
