use alumet::{
    measurement::{AttributeValue, MeasurementBuffer, MeasurementPoint, WrappedMeasurementValue},
    pipeline::{
        Transform,
        elements::{error::TransformError, transform::TransformContext},
    },
    units::{Unit, UnitPrefix},
};
use anyhow::anyhow;
use core::f64;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::Config;

pub struct EnergyEstimationTdpTransform {
    pub config: Config,
    pub metrics: super::Metrics,
    /// Cached CPU time conversion factor to ensure micro seconds
    conversion_factor: Option<f64>,
}

impl EnergyEstimationTdpTransform {
    /// Instantiates a new EnergyAttributionTransform with its private fields initialized.
    pub fn new(config: Config, metrics: super::Metrics) -> Self {
        Self {
            config,
            metrics,
            conversion_factor: None,
        }
    }
}

impl Transform for EnergyEstimationTdpTransform {
    /// Applies the transform on the measurements.
    fn apply(&mut self, measurements: &mut MeasurementBuffer, _ctx: &TransformContext) -> Result<(), TransformError> {
        // Retrieve the cpu_usage and energy_estimate metric's ids.
        // Using a nested scope to reduce the lock time.
        log::trace!("enter in apply transform function");
        // usage as time delta
        let cpu_usage_id = self.metrics.cpu_usage.as_u64();
        let metric_id = self.metrics.estimated_consumed_energy;

        log::trace!(
            "enter in apply transform function, number of measurements: {}",
            measurements.len()
        );
        // Return cached value if already computed. Fail if not compatible
        if self.conversion_factor.is_none() {
            let cpu_usage_metric = _ctx
                .metrics
                .by_id(&self.metrics.cpu_usage)
                .ok_or_else(|| TransformError::Fatal(anyhow!("cpu_usage metric not found in metric registry")))?;

            let conversion_factor = match (&cpu_usage_metric.unit.base_unit, &cpu_usage_metric.unit.prefix) {
                // Nanoseconds -> microseconds
                (Unit::Second, UnitPrefix::Nano) => 1.0 / 1000.0,
                // Microseconds -> microseconds
                (Unit::Second, UnitPrefix::Micro) => 1.0,
                // Milliseconds -> microseconds
                (Unit::Second, UnitPrefix::Milli) => 1000.0,
                // Seconds -> microseconds
                (Unit::Second, UnitPrefix::Plain) => 1_000_000.0,
                (base, prefix) => {
                    return Err(TransformError::UnexpectedInput(anyhow!(
                        "Unsupported cpu_usage unit for tdp plugin: {:?} with prefix {:?}.",
                        base,
                        prefix
                    )));
                }
            };

            log::debug!(
                "Detected cpu_usage unit: {:?} with prefix {:?}, conversion factor: {}",
                cpu_usage_metric.unit.base_unit,
                cpu_usage_metric.unit.prefix,
                conversion_factor
            );

            self.conversion_factor = Some(conversion_factor);
        }
        let conversion_factor = self.conversion_factor.unwrap();

        for point in measurements.clone().iter() {
            if point.metric.as_u64() == cpu_usage_id {
                let id = SystemTime::from(point.timestamp).duration_since(UNIX_EPOCH)?.as_secs();
                log::trace!("we get a measurement for pod with timestamp: {}", id);

                let value = match point.value {
                    WrappedMeasurementValue::F64(x) => x.to_string(),
                    WrappedMeasurementValue::U64(x) => x.to_string(),
                };

                // energy = cpu_usage * nb_vcpu/nb_cpu * tdp / poll_interval
                let mut estimated_energy = value.parse().unwrap();
                estimated_energy = estimated_energy * conversion_factor * self.config.nb_vcpu / self.config.nb_cpu
                    * self.config.tdp
                    / (1000000.0)
                    / (self.config.poll_interval.as_secs() as f64);

                log::trace!(
                    "we get a measurement with resource:{}",
                    point.resource.id_display().to_string()
                );
                log::trace!(
                    "we get a measurement with consumer:{}",
                    point.consumer.id_display().to_string()
                );
                log::trace!("we get a measurement with value:{}", value);
                log::trace!("estimate energy consumption:{}", estimated_energy);

                let point_attributes: Vec<(String, AttributeValue)> = point
                    .attributes()
                    .map(|(key, value)| (key.to_owned(), value.clone()))
                    .collect();

                // Sort the attributes by key
                for (key, value_attr) in &point_attributes {
                    log::trace!(
                        "read attribute key / value: {} / {}",
                        key.as_str(),
                        value_attr.to_string()
                    );
                }

                let new_m = MeasurementPoint::new(
                    point.timestamp,
                    metric_id,
                    point.resource.clone(),
                    point.consumer.clone(),
                    estimated_energy,
                )
                .with_attr_vec(point_attributes);
                measurements.push(new_m.clone());
            }
        }
        Ok(())
    }
}
