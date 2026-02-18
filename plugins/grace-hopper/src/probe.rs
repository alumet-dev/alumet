use std::{
    collections::HashMap,
    ops::{Add, Sub},
};

use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint, MeasurementType, Timestamp},
    metrics::TypedMetricId,
    pipeline::{Source, elements::error::PollError},
    resources::{Resource, ResourceConsumer},
};
use enum_map::EnumMap;

use crate::{
    Metrics,
    hwmon::{Device, SensorTagKind, TelemetryKind},
    total::PerKindTotals,
};

pub struct GraceHopperSource {
    /// Hwmon probes for each socket
    probes: HashMap<u8, Vec<Probe>>,
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

/// Helper to estimate the DRAM power.
///
/// ## Why?
///
/// Grace and Grace-hopper superchips do not provide a sensor that is dedicated to the RAM.
/// However, it is possible to estimation the power consumption of the RAM by using the existing sensors.
pub struct DramComputation<T>
where
    T: Copy + Add<Output = T> + Sub<Output = T> + PartialOrd,
{
    grace_value: Option<T>,
    cpu_value: Option<T>,
    sysio_value: Option<T>,
}

impl<T> DramComputation<T>
where
    T: Copy + Add<Output = T> + Sub<Output = T> + PartialOrd,
{
    /// Estimates the DRAM power and energy by computing `grace - cpu - sysio`.
    ///
    /// This is not the _best_ estimation possible, but it's good enough for the moment.
    /// It overestimates the consumption of the DRAM, because it contains the consumption of some controllers
    pub fn compute(&self) -> Option<T> {
        let grace = self.grace_value?;
        let cpu = self.cpu_value?;
        let sysio = self.sysio_value?;

        if (cpu + sysio) > grace {
            // Note: cannot use checked_sub because we want the method to accept both u64 and f64,
            // and checked_sub does not exist on f64.
            None
        } else {
            Some(grace - cpu - sysio)
        }
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
        // TODO shouldn't we change the timestamp?
        Ok(energy_consumed)
    }
}

impl GraceHopperSource {
    pub fn new(metrics: Metrics, devices: Vec<Device>) -> Self {
        let probes: HashMap<u8, Vec<Probe>> = devices.into_iter().fold(HashMap::new(), |mut sockets, d| {
            sockets.entry(d.info.socket).or_default().push(Probe::new(d));
            sockets
        });
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

        fn total_point<T: MeasurementType>(
            t: Timestamp,
            metric: TypedMetricId<T>,
            kind: Option<SensorTagKind>,
            value: T::T,
        ) -> MeasurementPoint {
            let m = MeasurementPoint::new(t, metric, Resource::LocalMachine, ResourceConsumer::LocalMachine, value);
            match kind {
                Some(kind) => m.with_attr("sensor", kind.as_str_total()),
                None => m.with_attr("sensor", "total"),
            }
        }

        // Compute the sum per kind of sensor.
        let mut total_power = PerKindTotals::new();
        let mut total_energy = PerKindTotals::new();

        // Collect all the powers and energies.
        for (socket, probes) in self.probes.iter_mut() {
            // Keep track of some values per-probe to estimate the dram consumption.
            let mut local_power: EnumMap<TelemetryKind, Option<u64>> = enum_map::enum_map! { _ => None};
            let mut local_energy: EnumMap<TelemetryKind, Option<f64>> = enum_map::enum_map! { _ => None};

            for probe in probes {
                let kind = probe.device.info.kind;
                let ProbeMeasure { power, energy } = probe.measure(t, &mut self.buf)?;

                measurements.push(probe_point(t, self.metrics.power, &probe.device, power));
                if let Some(energy) = energy {
                    measurements.push(probe_point(t, self.metrics.energy, &probe.device, energy));
                }

                local_power[kind] = Some(power);
                local_energy[kind] = energy;

                total_power.push(kind.into(), power);
                if let Some(energy) = energy {
                    total_energy.push(kind.into(), energy);
                }
            }

            // Estimate dram power consumption.
            let dram_power: DramComputation<u64> = DramComputation {
                grace_value: local_power[TelemetryKind::Grace],
                cpu_value: local_power[TelemetryKind::Cpu],
                sysio_value: local_power[TelemetryKind::SysIo],
            };
            if let Some(computed_dram_power) = dram_power.compute() {
                total_power.push(SensorTagKind::Dram, computed_dram_power);
                measurements.push(
                    MeasurementPoint::new(
                        t,
                        self.metrics.power,
                        Resource::Dram { pkg_id: *socket as u32 },
                        ResourceConsumer::LocalMachine,
                        computed_dram_power,
                    )
                    .with_attr("sensor", "dram"),
                );
            } else {
                log::warn!("could not compute dram power: missing inputs or overflow");
            }

            // Idem with energy.
            let dram_energy: DramComputation<f64> = DramComputation {
                grace_value: local_energy[TelemetryKind::Grace],
                cpu_value: local_energy[TelemetryKind::Cpu],
                sysio_value: local_energy[TelemetryKind::SysIo],
            };
            if let Some(computed_dram_energy) = dram_energy.compute() {
                total_energy.push(SensorTagKind::Dram, computed_dram_energy);
                measurements.push(
                    MeasurementPoint::new(
                        t,
                        self.metrics.energy,
                        Resource::Dram { pkg_id: *socket as u32 },
                        ResourceConsumer::LocalMachine,
                        computed_dram_energy,
                    )
                    .with_attr("sensor", "dram"),
                );
            } else {
                log::warn!("could not compute dram energy: missing inputs or overflow");
            }
        }

        // Push the totals
        for (kind, total) in total_power.iter() {
            measurements.push(total_point(t, self.metrics.power, Some(kind), total));
        }
        for (kind, total) in total_energy.iter() {
            measurements.push(total_point(t, self.metrics.energy, Some(kind), total));
        }

        // Find the total consumption of the superchip.
        // On GraceHopper superchips: the "module" power.
        // On Grace superchips: the sum of all the "grace" power (there is no "module" device).
        if let Some(total_power) = total_power[SensorTagKind::Module].or(total_power[SensorTagKind::Grace]) {
            measurements.push(total_point(t, self.metrics.power, None, total_power));
        }
        if let Some(total_energy) = total_energy[SensorTagKind::Module].or(total_energy[SensorTagKind::Grace]) {
            measurements.push(total_point(t, self.metrics.energy, None, total_energy));
        }
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        for (_, probes) in self.probes.iter_mut() {
            for probe in probes {
                probe.prev_power = None;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use alumet::measurement::Timestamp;
    use std::time::Duration;

    use crate::probe::DramComputation;

    use super::PowerMeasure;

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

    #[test]
    fn compute_dram() {
        let test1: DramComputation<u64> = DramComputation {
            grace_value: Some(19),
            sysio_value: Some(12),
            cpu_value: Some(2),
        };
        let mut computed_u64 = test1.compute();
        assert_eq!(computed_u64.unwrap(), 5);

        let test2: DramComputation<u64> = DramComputation {
            grace_value: Some(20),
            sysio_value: Some(13),
            cpu_value: Some(7),
        };
        computed_u64 = test2.compute();
        assert_eq!(computed_u64.unwrap(), 0);

        let test3: DramComputation<u64> = DramComputation {
            grace_value: Some(10),
            sysio_value: Some(12),
            cpu_value: Some(2),
        };
        computed_u64 = test3.compute();
        assert_eq!(computed_u64, None);

        let test4: DramComputation<f64> = DramComputation {
            grace_value: Some(10.0),
            sysio_value: Some(3.1),
            cpu_value: Some(5.0),
        };
        let mut computed_f64 = test4.compute();
        assert_eq!(computed_f64.unwrap(), 1.9);

        let test5: DramComputation<f64> = DramComputation {
            grace_value: Some(28.3),
            sysio_value: Some(13.3),
            cpu_value: Some(15.0),
        };
        computed_f64 = test5.compute();
        assert_eq!(computed_f64.unwrap(), 0.0);

        let test6: DramComputation<f64> = DramComputation {
            grace_value: Some(15.0),
            sysio_value: Some(12.58),
            cpu_value: Some(20.59),
        };
        computed_f64 = test6.compute();
        assert_eq!(computed_f64, None);
    }
}
