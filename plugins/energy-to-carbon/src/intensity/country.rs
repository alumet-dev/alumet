use super::EmissionIntensityProvider;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Configuration for the country-based emission intensity provider.
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct CountryConfig {
    /// Country code in ISO 3166-1 alpha-3 format (e.g. `"FRA"`, `"DEU"`).
    code: String,
}

/// Provides a static carbon emission intensity based on a country's energy mix.
///
/// The intensity is loaded once at initialization from a bundled JSON asset
/// (`assets/energy_mix_per_country.json`) and never changes at runtime.
pub struct CountryIntensityProvider {
    intensity: f64,
}

impl CountryIntensityProvider {
    /// Creates a new [`CountryIntensityProvider`] from the given configuration.
    pub fn new(country_config: CountryConfig) -> anyhow::Result<Self> {
        let code = country_config.code;
        if code.is_empty() {
            return Err(anyhow::anyhow!("country.code is required when mode is 'country'"));
        }
        // Embedded at compile time via `include_str!`, no runtime file I/O.
        let energy_mix = include_str!("../../assets/energy_mix_per_country.json");
        let deserialized_json: Value = serde_json::from_str(energy_mix)?;
        let intensity = deserialized_json[code.as_str()]["carbon_intensity"]
            .as_f64()
            .ok_or_else(|| anyhow::anyhow!("Country '{}' not found in energy mix file", code))?;
        Ok(Self { intensity: intensity })
    }
}

impl EmissionIntensityProvider for CountryIntensityProvider {
    /// Returns the country's carbon emission intensity in gCO₂/Wh.
    fn get_intensity(&self) -> anyhow::Result<f64> {
        Ok(self.intensity)
    }
}
