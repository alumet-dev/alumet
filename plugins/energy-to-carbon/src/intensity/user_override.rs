use super::EmissionIntensityProvider;
use serde::{Deserialize, Serialize};

/// Configuration for the user-override emission intensity provider.
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct OverrideConfig {
    /// Override the emission intensity value (in gCO₂/kWh).
    intensity: f64,
}

/// Provides a static carbon emission intensity from a user-supplied value.
pub struct OverrideIntensityProvider {
    intensity: f64,
}

impl OverrideIntensityProvider {
    /// Creates a new [`OverrideIntensityProvider`] from the given configuration.
    pub fn new(override_config: OverrideConfig) -> anyhow::Result<Self> {
        Ok(Self {
            intensity: override_config.intensity,
        })
    }
}

impl EmissionIntensityProvider for OverrideIntensityProvider {
    /// Returns the user-supplied carbon emission intensity in gCO₂/kWh.
    fn get_intensity(&self) -> anyhow::Result<f64> {
        Ok(self.intensity)
    }
}
