use core::f64;
use std::{
    clone, collections::HashMap, sync::{Arc, Mutex}, time::{SystemTime, UNIX_EPOCH},
};

use alumet::{
    measurement::{AttributeValue, MeasurementBuffer, MeasurementPoint, WrappedMeasurementValue},
    pipeline::{
        elements::{error::TransformError, transform::TransformContext},
        Transform,
    },
    resources::Resource,
};

use serde::de::value;

use crate::Config;

pub struct EnergyEstimationTdpTransform {
    pub config: Config,
    pub metrics: Arc<Mutex<super::Metrics>>,
}

impl EnergyEstimationTdpTransform {
    /// Instantiates a new EnergyAttributionTransform with its private fields initialized.
    pub fn new(config: Config, metrics: Arc<Mutex<super::Metrics>>) -> Self {
        Self {
            config,
            metrics,
        }
    }

}

impl Transform for EnergyEstimationTdpTransform {
    /// Applies the transform on the measurements.
    fn apply(&mut self, measurements: &mut MeasurementBuffer, _ctx: &TransformContext) -> Result<(), TransformError> {
        // Retrieve the pod_id and the rapl_id.
        // Using a nested scope to reduce the lock time.
        log::trace!("EZC: enter in apply transform function");

        let pod_id = {
            let metrics = self.metrics.lock().unwrap();

            let pod_id = metrics.cpu_usage_per_pod.unwrap().as_u64();
            pod_id
        };

        let metric_id = {
            let metrics = self.metrics.lock().unwrap();
            metrics.pod_estimate_attributed_energy.unwrap()
        };

        log::trace!("enter in apply transform function, number of measurements: {}",measurements.len());

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
                let mut estimated_energy = value.parse().unwrap();
                estimated_energy = estimated_energy*self.config.nb_vcpu/self.config.nb_cpu*self.config.tdp / (1000000 as f64) / (self.config.poll_interval.as_secs() as f64);

                log::trace!("we get a measurement with resource:{}", point.resource.id_display().to_string());
                log::trace!("we get a measurement with consummer:{}", point.consumer.id_display().to_string());
                log::trace!("we get a measurement with value:{}", value);
                log::trace!("estimate energy consumption:{}", estimated_energy);             
                
                let point_attributes: Vec<(String, AttributeValue)> = point
                .attributes()
                .map(|(key, value)| (key.to_owned(), value.clone()))
                .collect();

                // // Sort the attributes by key
                // let mut point_attributes = point.attributes().collect::<Vec<_>>();
                // attr_sorted.sort_by_key(|(k, _)| *k);

                for (key, valueAttr) in &point_attributes {
                    log::trace!("read attribute key / value: {} / {}", key.as_str(), valueAttr.to_string());
                    if (key.as_str().contains("node")) {
                        let node_value: String = valueAttr.to_string();
                        log::trace!("read attribute node value: {}", node_value);
                    }
                }
                
                let new_m = MeasurementPoint::new(
                    point.timestamp,
                    metric_id,
                    point.resource.clone(),
                    point.consumer.clone(),
                    estimated_energy).with_attr_vec(point_attributes);
                
                measurements.push(new_m.clone());

            }  
        }      
        Ok(())                                  
    }    
    
              

    }
        

