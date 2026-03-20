use crate::intensity::EmissionIntensityProvider;
use serde_json::Value;
use std::fs;

pub struct CountryIntensity {
    pub country: String,
    intensity: f64,
}

impl CountryIntensity {
    pub fn new(country: String) -> anyhow::Result<Self> {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/energy_mix_per_country.json");
        let energy_mix: String =
            fs::read_to_string(path).map_err(|e| anyhow::anyhow!("Failed to read energy mix file: {}", e))?;
        let deserialized_json: Value = serde_json::from_str(energy_mix.as_str())?;
        let intensity = deserialized_json[country.as_str()]["carbon_intensity"]
            .as_f64()
            .ok_or_else(|| anyhow::anyhow!("Country '{}' not found in energy mix file", country))?;
        Ok(Self { country, intensity })
    }
}

impl EmissionIntensityProvider for CountryIntensity {
    fn get_intensity(&self) -> anyhow::Result<f64> {
        Ok(self.intensity)
    }
}
