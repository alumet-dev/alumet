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

pub struct EnergyEstimationTdpTransform {
    pub metrics: Arc<Mutex<super::Metrics>>,
    buffer_pod: HashMap<u64, Vec<MeasurementPoint>>,
}

impl EnergyEstimationTdpTransform {
    /// Instantiates a new EnergyAttributionTransform with its private fields initialized.
    pub fn new(metrics: Arc<Mutex<super::Metrics>>) -> Self {
        Self {
            metrics,
            buffer_pod: HashMap::<u64, Vec<MeasurementPoint>>::new(),
        }
    }

}

impl Transform for EnergyEstimationTdpTransform {
    /// Applies the transform on the measurements.
    fn apply(&mut self, measurements: &mut MeasurementBuffer, _ctx: &TransformContext) -> Result<(), TransformError> {
        // Retrieve the pod_id and the rapl_id.
        // Using a nested scope to reduce the lock time.
        log::trace!("EZC: enter in apply transform function");

        // for 1st version, the tdp is hardcoded
        let tdp:f64 = 1.0;

        let pod_id = {
            let metrics = self.metrics.lock().unwrap();

            let pod_id = metrics.cpu_usage_per_pod.unwrap().as_u64();
            pod_id
        };

        let metric_id = {
            let metrics = self.metrics.lock().unwrap();
            metrics.pod_estimate_attributed_energy.unwrap()
        };

        log::trace!("EZC: enter in apply transform function, number of measurements: {}",measurements.len());

        for point in measurements.clone().iter() {

            if point.metric.as_u64() == pod_id {                
                let id = SystemTime::from(point.timestamp).duration_since(UNIX_EPOCH)?.as_secs();
                log::trace!("EZC: we get a measurement with timestamp: {}", id);

                let mut estimated_energy: f64 = 0.0;

                let value = match point.value {
                    WrappedMeasurementValue::F64(x) => x.to_string(),
                    WrappedMeasurementValue::U64(x) => x.to_string(),
                };

                // from k8s plugin we get the cpu_usage_per_pod in micro second
                // energy = cpu_usage_per_pod * tdp 
                estimated_energy = value.parse().unwrap();
                estimated_energy = estimated_energy*tdp;

                log::trace!("EZC: we get a measurement with resource:{}", point.resource.id_display().to_string());
                log::trace!("EZC: we get a measurement with consummer:{}", point.consumer.id_display().to_string());
                log::trace!("EZC: we get a measurement with value:{}", value);
                log::trace!("EZC: estimate energy consumption:{}", estimated_energy);             
                
                let point_attributes = point
                .attributes()
                .map(|(key, value)| (key.to_owned(), value.clone()))
                .collect();

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
        

