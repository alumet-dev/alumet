pub use crate::intensity::EmissionIntensityProvider;
use alumet::{
    measurement::{MeasurementBuffer, MeasurementPoint, WrappedMeasurementValue},
    metrics::RawMetricId,
    pipeline::{
        Transform,
        elements::{error::TransformError, transform::TransformContext},
    },
    units::{PrefixedUnit, Unit},
};

pub(crate) struct EnergyToCarbonTransform {
    pub(crate) carbon_emission: RawMetricId,
    pub(crate) emission_intensity_provider: Box<dyn EmissionIntensityProvider>,
}

impl Transform for EnergyToCarbonTransform {
    fn apply(
        &mut self,
        measurements: &mut MeasurementBuffer,
        _ctx: &TransformContext,
    ) -> std::result::Result<(), TransformError> {
        let mut carbon_points = Vec::new();

        for m in measurements.iter() {
            let metric = _ctx.metrics.by_id(&m.metric).unwrap();
            // If the metric is in <prefix>joules => convert to joules => transform to gCo2 => add it to `carbon_points`

            let mut factor: f64 = 0.0; // 0.0 means "not a joule unit"
           factor = match metric.unit {
              PrefixedUnit::Nano(Unit::Joule) => 1e-9,
              PrefixedUnit::Micro(Unit::Joule) => 1e-6,
              PrefixedUnit::Milli(Unit::Joule) => 1e-3,
              PrefixedUnit::Unit(Unit::Joule) => 1.0,
              PrefixedUnit::Kilo(Unit::Joule) => 1e3,
              PrefixedUnit::Mega(Unit::Joule) => 1e6,
              PrefixedUnit::Giga(Unit::Joule) => 1e9,
              _ => factor,
          };

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
                    WrappedMeasurementValue::F64(
                        energy * factor * self.emission_intensity_provider.get_intensity().unwrap(),
                    ),
                ));
            }
        }

        for point in carbon_points {
            measurements.push(point);
        }

        Ok(())
    }
}
