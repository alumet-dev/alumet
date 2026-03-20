mod intensity;
mod transform;

use alumet::{
    metrics::def::MetricId,
    plugin::{
        AlumetPluginStart, ConfigTable,
        rust::{AlumetPlugin, deserialize_config, serialize_config},
    },
    units::Unit,
};
use intensity::EmissionIntensityProvider;
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub struct EnergyToCarbonPlugin {
    config: Config,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
enum Mode {
    IntensityOverride,
    Country,
    WorldAvg,
}

#[derive(Serialize, Deserialize, Clone, Default)]
struct OverrideConfig {
    /// Override the emission intensity value (in gCO₂/kWh).
    intensity: f64,
}

#[derive(Serialize, Deserialize, Clone, Default)]
struct CountryConfig {
    /// Country 3-letter ISO code.
    code: String,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
struct Config {
    // "country", "intensity_override" or "world_avg"
    mode: Option<Mode>,
    // Other parameters
    #[serde(with = "humantime_serde")]
    poll_interval: Duration,
    #[serde(rename = "intensity_override")]
    override_config: OverrideConfig,
    #[serde(rename = "country")]
    country_config: CountryConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mode: None,
            override_config: OverrideConfig::default(),
            country_config: CountryConfig::default(),
            poll_interval: Duration::from_secs(1),
        }
    }
}

impl AlumetPlugin for EnergyToCarbonPlugin {
    fn name() -> &'static str {
        "energy-to-carbon"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        let config = serialize_config(Config::default())?;
        Ok(Some(config))
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config)?;
        Ok(Box::new(EnergyToCarbonPlugin { config }))
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        let provider: Box<dyn EmissionIntensityProvider> = match self.config.mode {
            Some(Mode::IntensityOverride) => {
                Box::new(intensity::OverrideIntensity(self.config.override_config.intensity))
            }
            Some(Mode::Country) => {
                let code = &self.config.country_config.code;
                if code.is_empty() {
                    return Err(anyhow::anyhow!("country.code is required when mode is 'country'"));
                }
                Box::new(intensity::CountryIntensity::new(code.clone())?)
            }
            Some(Mode::WorldAvg) => Box::new(intensity::WorldAvgIntensity),
            None => {
                return Err(anyhow::anyhow!(
                    "You must specify a mode: 'intensity_override', 'country', or 'world_avg'"
                ));
            }
        };

        let carbon_emission = alumet.create_metric::<f64>(
            "carbon_emission",
            Unit::Custom {
                unique_name: "g_CO2".to_string(),
                display_name: "gCO₂".to_string(),
            },
            "Carbon emission in grams of CO2 equivalent, computed from energy consumption and emission intensity.",
        )?;

        // Create the transform
        let transform = transform::EnergyToCarbonTransform {
            carbon_emission: carbon_emission.untyped_id(),
            emission_intensity_provider: provider,
        };

        // Add the transform to the measurement pipeline
        let _ = alumet.add_transform("transform", Box::new(transform));

        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}
