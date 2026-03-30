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
use intensity::{
    EmissionIntensityProvider,
    country::{CountryConfig, CountryIntensityProvider},
    user_override::{OverrideConfig, OverrideIntensityProvider},
    world_avg::WorldAvgIntensityProvider,
};
use serde::{Deserialize, Serialize};

/// Alumet plugin that converts energy measurements into carbon emission estimates.
pub struct EnergyToCarbonPlugin {
    config: Config,
}

/// Determines which emission intensity provider to use.
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
enum Mode {
    /// Use a fixed intensity value supplied by the user.
    IntensityOverride,
    /// Look up the intensity for a specific country.
    Country,
    /// Use the world average intensity.
    WorldAvg,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
struct Config {
    mode: Mode,
    #[serde(rename = "intensity_override")]
    override_config: Option<OverrideConfig>,
    #[serde(rename = "country")]
    country_config: Option<CountryConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mode: Mode::WorldAvg,
            override_config: None,
            country_config: None,
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

    /// Registers the `carbon_emission` metric and adds the energy-to-carbon transform
    /// to the pipeline.
    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        let provider: Box<dyn EmissionIntensityProvider> =
            match self.config.mode {
                Mode::IntensityOverride => {
                    let cfg =
                        self.config.override_config.clone().ok_or_else(|| {
                            anyhow::anyhow!("missing [plugin.energy-to-carbon.override] config section")
                        })?;
                    Box::new(OverrideIntensityProvider::new(cfg)?)
                }
                Mode::Country => {
                    let cfg =
                        self.config.country_config.clone().ok_or_else(|| {
                            anyhow::anyhow!("missing [plugin.energy-to-carbon.country] config section")
                        })?;
                    Box::new(CountryIntensityProvider::new(cfg)?)
                }
                Mode::WorldAvg => Box::new(WorldAvgIntensityProvider::new()?),
            };

        let carbon_emission = alumet.create_metric::<f64>(
            "carbon_emission",
            Unit::Custom {
                unique_name: "g_CO2".to_string(),
                display_name: "gCO₂".to_string(),
            },
            "Carbon emission in grams of CO2 equivalent, computed from energy consumption and emission intensity.",
        )?;

        let transform = transform::EnergyToCarbonTransform::new(carbon_emission.untyped_id(), provider)?;
        let _ = alumet.add_transform("transform", Box::new(transform));

        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}
