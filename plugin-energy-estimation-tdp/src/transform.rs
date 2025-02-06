use core::f64;
use std::time::{SystemTime, UNIX_EPOCH};

use alumet::{
    measurement::{AttributeValue, MeasurementBuffer, MeasurementPoint, WrappedMeasurementValue},
    pipeline::{
        elements::{error::TransformError, transform::TransformContext},
        Transform,
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
        // Retrieve the pod_id and the rapl_id.
        // Using a nested scope to reduce the lock time.
        log::trace!("enter in apply transform function");

        // Harcoded ram energy consumption in Watts
        let ram_consumption_avrg = 3.0;

        let pod_id = self.metrics.system_cpu_usage.as_u64();
        let metric_id = self.metrics.system_estimated_energy_consumption;

        log::trace!(
            "enter in apply transform function, number of measurements: {}",
            measurements.len()
        );

        for point in measurements.clone().iter() {
            if point.metric.as_u64() == pod_id {
                let id = SystemTime::from(point.timestamp).duration_since(UNIX_EPOCH)?.as_secs();
                log::trace!("we get a measurement for pod with timestamp: {}", id);

                let value = match point.value {
                    WrappedMeasurementValue::F64(x) => x.to_string(),
                    WrappedMeasurementValue::U64(x) => x.to_string(),
                };

                // from k8s plugin we get the cpu_usage_per_pod in micro second
                // energy = cpu_usage_per_pod * nb_vcpu/nb_cpu * tdp / poll_interval
                let kernel_cpu_time: f64 = value.parse().unwrap();
                let estimated_energy =
                    self.config.tdp * (kernel_cpu_time / (self.config.nb_cpu + self.config.nb_vcpu)) / 1000.0;

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

                let mut new_m = MeasurementPoint::new(
                    point.timestamp,
                    metric_id,
                    point.resource.clone(),
                    point.consumer.clone(),
                    0.0,
                )
                .with_attr_vec(point_attributes.clone());

                if point_attributes
                    .clone()
                    .contains(&(("cpu_state".to_string()), AttributeValue::String("idle".to_string())))
                {
                    let value = match point.value {
                        WrappedMeasurementValue::F64(x) => x,
                        WrappedMeasurementValue::U64(x) => x as f64,
                    };
                    let cpu_usage = 1.0 - (value) / (self.config.nb_cpu + self.config.nb_vcpu);
                    if cpu_usage >= 0.5 {
                        new_m.value = WrappedMeasurementValue::F64(self.config.tdp);
                    } else {
                        new_m.value = WrappedMeasurementValue::F64(cpu_usage * self.config.tdp + ram_consumption_avrg);
                    }
                }

                measurements.push(new_m.clone());
            }
        }
        Ok(())
    }
}
