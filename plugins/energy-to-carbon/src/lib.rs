mod intensity;
mod transform;

// Clean imports ⬇️
use std::{time::Duration, fs};
use alumet::{
    units::{Unit, PrefixedUnit},
    measurement::{MeasurementAccumulator, MeasurementPoint, Timestamp, WrappedMeasurementValue, MeasurementBuffer},
    metrics::{TypedMetricId, RawMetricId, def::MetricId},
    pipeline::{
        Transform,
        Source,
        elements::{
            error::{PollError,TransformError},
            transform::TransformContext,
            source::trigger,
        },
    },
    plugin::{
        AlumetPluginStart, ConfigTable,
        rust::{AlumetPlugin, serialize_config, deserialize_config},
    },
    resources::{Resource, ResourceConsumer},
};
use serde::{Serialize, Deserialize};
use serde_json::Value;


pub struct EnergyToCarbonPlugin{
    config: Config,
}

#[derive(Serialize, Deserialize, Clone, Default)]
struct OverrideConfig {
    /// Override the emission intensity value (in gCO₂/kWh).
    intensity: Option<f64>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
struct CountryConfig {
    /// Country 3-letter ISO code.
    code: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
struct Config {
    /// Cascading parameters used to set emission intensity
    mode: Option<String>,
    // Other parameters
    #[serde(with = "humantime_serde")]
    poll_interval: Duration,
    #[serde(rename = "override")]
    override_config: OverrideConfig,  //optionnel
    country: CountryConfig,  //optionnel
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mode: None,
            override_config: OverrideConfig::default(),
            country: CountryConfig::default(),
            poll_interval: Duration::from_secs(1),
        }
    }
}


impl AlumetPlugin for EnergyToCarbonPlugin {
    fn name() -> &'static str {
        "energy-to-carbon" // the name of your plugin, in lowercase, without the "plugin-" prefix
    }

    fn version() -> &'static str {
        log::info!("Version here!!!");
        env!("CARGO_PKG_VERSION") // gets the version from the Cargo.toml of the plugin crate
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        let config = serialize_config(Config::default())?;
        Ok(Some(config))
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        log::info!("Init here!!!");
        let config = deserialize_config(config)?;
        Ok(Box::new(EnergyToCarbonPlugin { config }))
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        log::info!("Start here!!");
        // emission_intensity mode

        let provider: Box<dyn EmissionIntensityProvider> = match self.config.mode.as_deref() {
            Some("override")   => Box::new(OverrideIntensity(self.config.override_config.intensity.unwrap())),
            Some("country")    => Box::new(CountryIntensity(self.config.country.code.clone().unwrap())),
            Some("world_avg")  => Box::new(WorldAvgIntensity),
            Some(invalid)      => return Err(anyhow::anyhow!("{} is not a valid mode. Choose override, country or world_avg", invalid)),
            None               => return Err(anyhow::anyhow!("You need to choose a mode: override, country or world_avg")),
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
        let transform = EnergyToCarbonTransform {
            carbon_emission: carbon_emission.untyped_id(),
            emission_intensity_provider: provider,
        };

        // Add the transform to the measurement pipeline
        let _ = alumet.add_transform("transform", Box::new(transform));

        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        log::info!("Bye!!");
        Ok(())
    }
}