use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use alumet::{
    measurement::{MeasurementBuffer, MeasurementPoint, WrappedMeasurementValue},
    pipeline::{
        elements::{error::TransformError, transform::TransformContext},
        Transform,
    },
    resources::Resource,
};

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

    /// Empties the buffers and send the energy attribution points to the MeasurementBuffer.
    fn buffer_bouncer(&mut self, measurements: &mut alumet::measurement::MeasurementBuffer) {
        // Retrieving the metric_id of the energy attribution.
        // Using a nested scope to reduce the lock time.
        log::trace!("EZC: enter in buffer_bouncer");
        let metric_id = {
            let metrics = self.metrics.lock().unwrap();
            metrics.pod_estimate_attributed_energy.unwrap()
        };

        // If the buffers do have enough (every) MeasurementPoints,
        // then we compute the estimate energy attribution.
        while self.buffer_pod.len() >= 2 {
            
            // Compute the sum of every `total_usage_usec` for the given timestamp: `rapl_mini_id`.
            //let pod_point = self.buffer_pod.remove(1).unwrap();
                        
            // Then for every points in the buffer_pod estimate the power consumption
            for (key, point) in &self.buffer_pod {
            log::trace!("EZC: read pod cpu usage, key: {}", key);
            // let cur_tot_time_f64 = match point.value {
            //     WrappedMeasurementValue::F64(fx) => fx,
            //     WrappedMeasurementValue::U64(ux) => ux as f64,
            // };

            log::trace!("EZC: read 1 point");
            }
        }
    }        
}

impl Transform for EnergyEstimationTdpTransform {
    /// Applies the transform on the measurements.
    fn apply(&mut self, measurements: &mut MeasurementBuffer, _ctx: &TransformContext) -> Result<(), TransformError> {
        // Retrieve the pod_id and the rapl_id.
        // Using a nested scope to reduce the lock time.
        log::trace!("EZC: enter in apply transform function");

        log::trace!("EZC: enter in apply transform function: {}",self.buffer_pod.len());

        if self.buffer_pod.len() > 0
        {
            //read the pod_id
            let metrics = self.metrics.lock().unwrap();
            let pod_id = metrics.cpu_usage_per_pod.unwrap().as_u64();
            log::trace!("EZC: pode_id: {}", pod_id);
        }
        
        
        // Emptying the buffers and pushing the energy attribution to the MeasurementBuffer
        //self.buffer_bouncer(measurements);

        Ok(())
    }
}