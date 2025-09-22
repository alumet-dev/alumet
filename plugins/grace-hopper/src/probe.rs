use std::{
    fs::File,
    io::{Read, Seek},
};

use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint, MeasurementType, Timestamp},
    metrics::TypedMetricId,
    pipeline::{Source, elements::error::PollError},
    resources::{Resource, ResourceConsumer},
};

use crate::{
    Metrics,
    hwmon::{Device, TelemetryKind},
};

pub struct GraceHopperSource {
    probes: Vec<Probe>,
    metrics: Metrics,
    buf: String,
}

// #[derive(Debug)]
pub struct Probe {
    /// Hwmon device that provides power data
    device: Device,
    /// The previous power measure on this device, to compute the energy
    prev_power: Option<PowerMeasure>,
}

impl Probe {
    fn new(device: Device) -> Self {
        Self {
            device,
            prev_power: None,
        }
    }

    fn measure(&mut self, t: Timestamp, buf: &mut String) -> anyhow::Result<ProbeMeasure> {
        let power = self.device.read_power_value(buf)?;
        let m = PowerMeasure { t, power };
        let energy = match self.prev_power.as_mut() {
            Some(prev) => Some(m.compute_energy(prev)?),
            None => None,
        };
        self.prev_power = Some(m);
        Ok(ProbeMeasure { power, energy })
    }
}

struct ProbeMeasure {
    power: u64,
    energy: Option<f64>,
}

struct PowerMeasure {
    t: Timestamp,
    power: u64,
}

impl PowerMeasure {
    /// Computes an energy from a power, using as time the time elapsed between
    /// the current timestamp and the previous timestamp.
    ///
    /// This function first computes the time elapsed between two timestamps.
    /// It return an error if it's not possible
    /// The energy is computed using a discrete integral with the formula: Energy(J) = ((Power_old(W) + Power_new(W)) / 2) * Time(s)
    fn compute_energy(&self, previous: &PowerMeasure) -> anyhow::Result<f64> {
        let time_elapsed = self.t.duration_since(previous.t)?.as_secs_f64();
        let energy_consumed = ((self.power + previous.power) as f64 / (2.0 * 1000.0)) * time_elapsed; // 1000 because we go from µW to mJ
        println!(
            "(power {} + previous {}) / 2   * time {} ({:?} - {:?})",
            self.power, previous.power, time_elapsed, self.t, previous.t
        );
        Ok(energy_consumed)
    }
}

impl GraceHopperSource {
    pub fn new(metrics: Metrics, devices: Vec<Device>) -> Self {
        let probes = devices.into_iter().map(Probe::new).collect();
        Self {
            probes,
            metrics,
            buf: String::with_capacity(8),
        }
    }
}

impl Source for GraceHopperSource {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, t: Timestamp) -> Result<(), PollError> {
        fn probe_point<T: MeasurementType>(
            t: Timestamp,
            metric: TypedMetricId<T>,
            dev: &Device,
            value: T::T,
        ) -> MeasurementPoint {
            MeasurementPoint::new(
                t,
                metric,
                Resource::CpuPackage {
                    id: dev.info.socket as u32,
                },
                ResourceConsumer::LocalMachine,
                value,
            )
            .with_attr("sensor", dev.info.kind.as_str())
        }

        // Compute some sums. One of grace/module is the total consumption of the superchip.
        let mut total_power_grace: Option<u64> = None;
        let mut total_power_module: Option<u64> = None;
        let mut total_energy_grace: Option<f64> = None;
        let mut total_energy_module: Option<f64> = None;

        // Collect all the powers and energies.
        for probe in self.probes.iter_mut() {
            let ProbeMeasure { power, energy } = probe.measure(t, &mut self.buf)?;

            measurements.push(probe_point(t, self.metrics.power, &probe.device, power));
            if let Some(energy) = energy {
                measurements.push(probe_point(t, self.metrics.energy, &probe.device, energy));
            }

            match probe.device.info.kind {
                TelemetryKind::Grace => {
                    *total_power_grace.get_or_insert_default() += power;
                    if let Some(energy) = energy {
                        *total_energy_grace.get_or_insert_default() += energy;
                    }
                }
                TelemetryKind::Module => {
                    *total_power_module.get_or_insert_default() += power;
                    if let Some(energy) = energy {
                        *total_energy_module.get_or_insert_default() += energy;
                    }
                }
                _ => (),
            }
        }

        // Find the total consumption.
        // On GraceHopper superchips: the "module" power.
        // On Grace superchips: the sum of all the "grace" power (there is no "module" device).
        if let Some(total_power) = total_power_module.or(total_power_grace) {
            measurements.push(
                MeasurementPoint::new(
                    t,
                    self.metrics.power,
                    Resource::LocalMachine,
                    ResourceConsumer::LocalMachine,
                    total_power,
                )
                .with_attr("sensor", "total"),
            );
        }
        if let Some(total_energy) = total_energy_module.or(total_energy_grace) {
            measurements.push(
                MeasurementPoint::new(
                    t,
                    self.metrics.energy,
                    Resource::LocalMachine,
                    ResourceConsumer::LocalMachine,
                    total_energy,
                )
                .with_attr("sensor", "total"),
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
    use std::fs::File;
    use std::io::Write;
    use std::time::Duration;
    use tempfile::tempdir;

    use super::{PowerMeasure, read_power_value};

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
            let mut file = File::open(&file_path).expect("failed to open the file");
            let mut buffer = String::new();
            let result = read_power_value(&mut buffer, &mut file);
            let power = result.expect(&format!("expected ok for input {}", line));
            assert_eq!(power, expected_sensor, "Incorrect sensor for input '{}'", line);
        }
    }

    #[test]
    fn compute_energy_from_power() {
        let ts0 = Timestamp::now();
        let mut lm_init = PowerMeasure { t: ts0, power: 0 };
        // timestamp diff is 0, can't compute energy -> 0
        let mut measure = PowerMeasure {
            t: ts0,
            power: 140_000000,
        };
        assert_eq!(0.0, measure.compute_energy(&lm_init).unwrap());
        lm_init.power = measure.power;

        let ts6 = ts0 + Duration::from_secs(6);
        measure = PowerMeasure {
            t: ts6,
            power: 25_000000,
        };
        assert_eq!(495_000.0, measure.compute_energy(&lm_init).unwrap());
        lm_init.power = measure.power;
        lm_init.t = measure.t;

        lm_init.t = ts0 + Duration::from_secs(5);
        lm_init.power = 70_000000;
        let ts55 = ts0 + Duration::from_millis(5500);
        measure = PowerMeasure {
            t: ts55,
            power: 130_000000,
        };
        assert_eq!(50_000.0, measure.compute_energy(&lm_init).unwrap());
        lm_init.t = measure.t;

        lm_init.t = lm_init.t + Duration::from_millis(500);
        lm_init.power = 50_000000;
        let ts10 = ts0 + Duration::from_secs(10);
        measure = PowerMeasure {
            t: ts10,
            power: 75_000000,
        };
        assert_eq!(250_000.0, measure.compute_energy(&lm_init).unwrap());

        lm_init.t = ts0 + Duration::from_secs(9);
        lm_init.power = 80_000000;
        let ts97 = ts0 + Duration::from_millis(9700);
        measure = PowerMeasure {
            t: ts97,
            power: 63_000000,
        };
        assert_eq!(50_050.0, measure.compute_energy(&lm_init).unwrap());

        lm_init.t = ts0 + Duration::from_secs(15);
        lm_init.power = 70_000000;
        let ts19 = ts0 + Duration::from_secs(19);
        measure = PowerMeasure {
            t: ts19,
            power: 71_000000,
        };
        assert_eq!(282_000.0, measure.compute_energy(&lm_init).unwrap());
    }
}
