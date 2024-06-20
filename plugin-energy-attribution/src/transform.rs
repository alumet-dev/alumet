use std::sync::{Arc, Mutex};

use alumet::pipeline::Transform;

pub struct EnergyAttributionTransform {
    pub metrics: Arc<Mutex<super::Metrics>>,
}

impl Transform for EnergyAttributionTransform {
    fn apply(
        &mut self,
        measurements: &mut alumet::measurement::MeasurementBuffer,
    ) -> Result<(), alumet::pipeline::TransformError> {
        let metrics = self.metrics.lock().unwrap();
        
        todo!("attribution")
    }
}
