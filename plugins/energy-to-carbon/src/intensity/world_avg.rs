use super::EmissionIntensityProvider;

/// Provides a static carbon emission intensity based on the world average.
///
/// Uses a hardcoded value of 475 gCO₂/kWh, as reported by the
/// [IEA](https://www.iea.org/reports/global-energy-co2-status-report).
pub struct WorldAvgIntensityProvider {
    intensity: f64,
}

impl WorldAvgIntensityProvider {
    /// Creates a new [`WorldAvgIntensityProvider`].
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self { intensity: 475.0 })
    }
}

impl EmissionIntensityProvider for WorldAvgIntensityProvider {
    /// Returns the world average carbon emission intensity (475 gCO₂/kWh).
    fn get_intensity(&self) -> anyhow::Result<f64> {
        Ok(self.intensity)
    }
}
