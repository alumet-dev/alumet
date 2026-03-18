use std::fs;
use serde_json::Value;
use crate::intensity::EmissionIntensityProvider;


pub struct CountryIntensity(pub String);
impl EmissionIntensityProvider for CountryIntensity {
    fn get_intensity(&self) -> anyhow::Result<f64> {
        // dynamic path to the json
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/intensity/country/energy_mix._per_country.json"
        );
        // Json file => String => Value
        let energy_mix: String = fs::read_to_string(path)
                .map_err(|e| anyhow::anyhow!("Failed to read energy mix file: {}", e))?;
        let deserialized_json: Value = serde_json::from_str(energy_mix.as_str())?;
        // Return the carbon_intensity 
        deserialized_json[&self.0.as_str()]["carbon_intensity"]
            .as_f64()
            .ok_or_else(|| anyhow::anyhow!("Country '{}' not found in energy mix file", self.0))
    }
}