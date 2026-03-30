pub use crate::intensity::EmissionIntensityProvider;
use alumet::{
    measurement::{MeasurementBuffer, MeasurementPoint, WrappedMeasurementValue},
    metrics::RawMetricId,
    pipeline::{
        Transform,
        elements::{error::TransformError, transform::TransformContext},
    },
    units::{Unit, UnitPrefix},
};

const JOULES_PER_KWH: f64 = 3.6e6;

/// Alumet transform that converts energy measurements into carbon emission estimates.
///
/// For each measurement in joules (any SI prefix), computes:
/// `carbon (gCO₂) = energy (J) × intensity (gCO₂/Wh) × unit_factor`
/// and appends the result as a new measurement point to the buffer.
pub struct EnergyToCarbonTransform {
    /// Metric ID for the output carbon emission measurement.
    carbon_emission: RawMetricId,
    /// Provider used to retrieve the carbon emission intensity.
    emission_intensity_provider: Box<dyn EmissionIntensityProvider>,
    /// Cached intensity value for static providers, `None` for dynamic ones.
    static_intensity: Option<f64>,
}

impl EnergyToCarbonTransform {
    /// Creates a new [`EnergyToCarbonTransform`].
    ///
    /// If the provider is static, the intensity is fetched once here and cached
    /// for the lifetime of the transform. Otherwise it is fetched on every batch.
    pub fn new(
        carbon_emission: RawMetricId,
        emission_intensity_provider: Box<dyn EmissionIntensityProvider>,
    ) -> anyhow::Result<Self> {
        let static_intensity = if emission_intensity_provider.is_intensity_static() {
            Some(emission_intensity_provider.get_intensity()?)
        } else {
            None
        };
        Ok(EnergyToCarbonTransform {
            carbon_emission,
            emission_intensity_provider,
            static_intensity,
        })
    }
}

impl Transform for EnergyToCarbonTransform {
    /// Converts all joule-based measurements in `measurements` to gCO₂ estimates
    /// and appends them to the same buffer alongside the original joule measurements.
    /// Non-joule measurements are left untouched.
    fn apply(
        &mut self,
        measurements: &mut MeasurementBuffer,
        _ctx: &TransformContext,
    ) -> std::result::Result<(), TransformError> {
        let intensity = match self.static_intensity {
            Some(v) => v,
            None => self
                .emission_intensity_provider
                .get_intensity()
                .map_err(|e| anyhow::anyhow!("Cannot get intensity: {}", e))?,
        };

        let mut carbon_points = MeasurementBuffer::new();
        for m in measurements.iter() {
            let metric = _ctx.metrics.by_id(&m.metric).unwrap();
            // Only process joule-based metrics; skip everything else.
            if metric.unit.base_unit != Unit::Joule {
                continue;
            }

            // Scale factor to convert the prefixed joule value to plain joules.
            let factor = match &metric.unit.prefix {
                UnitPrefix::Nano => 1e-9,
                UnitPrefix::Micro => 1e-6,
                UnitPrefix::Milli => 1e-3,
                UnitPrefix::Plain => 1.0,
                UnitPrefix::Kilo => 1e3,
                UnitPrefix::Mega => 1e6,
                UnitPrefix::Giga => 1e9,
            };

            let energy = match m.value {
                WrappedMeasurementValue::F64(v) => v,
                WrappedMeasurementValue::U64(v) => v as f64,
            };

            // Carry all attributes from the source joule measurement over to the carbon point.
            let attrs: Vec<_> = m.attributes().map(|(k, v)| (k.to_owned(), v.clone())).collect();

            let point = MeasurementPoint::new_untyped(
                m.timestamp,
                self.carbon_emission,
                m.resource.clone(),
                m.consumer.clone(),
                WrappedMeasurementValue::F64(energy / JOULES_PER_KWH * factor * intensity),
            )
            .with_attr_vec(attrs);

            carbon_points.push(point);
        }

        measurements.merge(&mut carbon_points);

        Ok(())
    }
}
