use core::f64;
use std::time::{SystemTime, UNIX_EPOCH};

use alumet::{
    measurement::{AttributeValue, MeasurementBuffer, MeasurementPoint, WrappedMeasurementValue},
    pipeline::{
        Transform,
        elements::{error::TransformError, transform::TransformContext},
    },
};

use crate::Config;

pub struct EnergyEstimationTdpTransform {
    pub config: Config,
    pub metrics: super::Metrics,
}

impl EnergyEstimationTdpTransform {
    /// Instantiates a new EnergyAttributionTransform with its private fields initialized.
    pub fn new(config: Config, metrics: super::Metrics) -> Self {
        Self { config, metrics }
    }
}

impl Transform for EnergyEstimationTdpTransform {
    /// Applies the transform on the measurements.
    fn apply(&mut self, measurements: &mut MeasurementBuffer, _ctx: &TransformContext) -> Result<(), TransformError> {
        // Retrieve the cpu_usage and energy_estimate metric's ids.
        // Using a nested scope to reduce the lock time.
        log::trace!("enter in apply transform function");
        // usage as time delta
        let cpu_usage_id = self.metrics.cpu_usage_per_domain.as_u64();
        let metric_id = self.metrics.domain_estimate_energy;

        log::trace!(
            "enter in apply transform function, number of measurements: {}",
            measurements.len()
        );

        for point in measurements.clone().iter() {
            if point.metric.as_u64() == cpu_usage_id {
                let id = SystemTime::from(point.timestamp).duration_since(UNIX_EPOCH)?.as_secs();
                log::trace!("we get a measurement for pod with timestamp: {}", id);

                let value = match point.value {
                    WrappedMeasurementValue::F64(x) => x.to_string(),
                    WrappedMeasurementValue::U64(x) => x.to_string(),
                };

                // from k8s plugin we get the cpu_usage_percent in micro second
                // energy = cpu_usage_percent * nb_vcpu/nb_cpu * tdp / poll_interval
                let mut estimated_energy = value.parse().unwrap();
                estimated_energy = estimated_energy * self.config.nb_vcpu / self.config.nb_cpu * self.config.tdp
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
                    if key.as_str().contains("node") {
                        let node_value: String = value_attr.to_string();
                        log::trace!("read attribute node value: {}", node_value);
                    }
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
