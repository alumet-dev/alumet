pub use crate::intensity::EmissionIntensityProvider;
use alumet::{
    measurement::{MeasurementBuffer, MeasurementPoint, WrappedMeasurementValue},
    metrics::RawMetricId,
    pipeline::{
        Transform,
        elements::{error::TransformError, transform::TransformContext},
    },
    units::{PrefixedUnit, Unit, UnitPrefix},
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
           factor = match (&metric.unit.prefix, &metric.unit.base_unit) {
                (UnitPrefix::Nano,  Unit::Joule) => 1e-9,
                (UnitPrefix::Micro, Unit::Joule) => 1e-6,
                (UnitPrefix::Milli, Unit::Joule) => 1e-3,
                (UnitPrefix::Plain,  Unit::Joule) => 1.0,
                (UnitPrefix::Kilo,  Unit::Joule) => 1e3,
                (UnitPrefix::Mega,  Unit::Joule) => 1e6,
                (UnitPrefix::Giga,  Unit::Joule) => 1e9,
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
