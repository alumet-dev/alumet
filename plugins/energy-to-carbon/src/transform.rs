use alumet::{
    measurement::{MeasurementBuffer, MeasurementPoint, WrappedMeasurementValue},
    metrics::RawMetricId,
    pipeline::{
        Transform,
        elements::{error::TransformError, transform::TransformContext},
    },
    units::{Unit, PrefixedUnit},
};
use crate::intensity::EmissionIntensityProvider;


struct EnergyToCarbonTransform {
    carbon_emission: RawMetricId,
    emission_intensity_provider: Box<dyn EmissionIntensityProvider>,
}

impl Transform for EnergyToCarbonTransform {
    fn apply(&mut self, measurements: &mut MeasurementBuffer, _ctx: &TransformContext) -> std::result::Result<(), TransformError> {
        // self.emission_intensity_provider.get_intensity().unwrap()
        let mut carbon_points = Vec::new();

        for m in measurements.iter() {
            let metric = _ctx.metrics.by_id(&m.metric).unwrap();
            // If the metric is in <prefix>joules => convert to joules => transform to gCo2 => add it to `carbon_points`

           let mut factor: f64 = 0.0; // 0.0 means "not a joule unit"
            match &metric.unit {
                u if *u == PrefixedUnit::nano(Unit::Joule)   => factor = 1e-9,
                u if *u == PrefixedUnit::micro(Unit::Joule)  => factor = 1e-6,
                u if *u == PrefixedUnit::milli(Unit::Joule)  => factor = 1e-3,
                u if *u == PrefixedUnit::from(Unit::Joule)   => factor = 1.0,
                u if *u == PrefixedUnit::kilo(Unit::Joule)   => factor = 1e3,
                u if *u == PrefixedUnit::mega(Unit::Joule)   => factor = 1e6,
                u if *u == PrefixedUnit::giga(Unit::Joule)   => factor = 1e9,
                _ => {}
            }

            if factor != 0.0 {
                let energy = match m.value {
                    WrappedMeasurementValue::F64(v) => v,
                    WrappedMeasurementValue::U64(v) => v as f64,
                };

                carbon_points.push(MeasurementPoint::new_untyped(
                    m.timestamp,
                    self.carbon_emission,
                    m.resource.clone(),
                    m.consumer.clone(),
                    // ! need to call get_intensity() at every apply, even if the value is fixed
                    WrappedMeasurementValue::F64(energy * factor * self.emission_intensity_provider.get_intensity().unwrap()),
                ));
            } 
        }
        
        for point in carbon_points {
            measurements.push(point);
        }

        Ok(())
    
    }
}