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

// mod transform;
// (in the code) transform::EnergyToCarbonTransform


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

trait EmissionIntensityProvider: Send {
    fn get_intensity(&self) -> anyhow::Result<f64>;
}

struct OverrideIntensity(f64);
impl EmissionIntensityProvider for OverrideIntensity {
    fn get_intensity(&self) -> anyhow::Result<f64> {
        Ok(self.0)
    }
}

struct CountryIntensity(String);
impl EmissionIntensityProvider for CountryIntensity {
    fn get_intensity(&self) -> anyhow::Result<f64> {
        // dynamic path to the json
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/resssources/energy_mix._per_country.json"
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


struct WorldAvgIntensity;
impl EmissionIntensityProvider for WorldAvgIntensity {
    fn get_intensity(&self) -> anyhow::Result<f64> {
        Ok(475.0)
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

        // // Test metric
        // let energy = alumet.create_metric::<f64> (
        //     "exemple_energy",
        //     Unit::Joule,
        //     "42.123j sent every seconds (for testing only)",
        // )?;

        // let energy_prefixed = alumet.create_metric::<f64> (
        //     "exemple_energy_prefixed",
        //     PrefixedUnit::milli(Unit::Joule),
        //     "31415mj sent every seconds (for testing only)",
        // )?;

        // // Test metric
        // let not_energy = alumet.create_metric::<u64> (
        //     "exemple_not_energy",
        //     Unit::Second,
        //     "2s sent every seconds (for testing only)",
        // )?;

        // // create the sources
        // let source_energy = ExampleSource {
        //     metric_energy: energy,
        //     metric_energy_prefixed: energy_prefixed,
        //     metric_not_energy: not_energy,
        // };

        // // How the source is triggered
        // let trigger_s = trigger::builder::time_interval(self.config.poll_interval).build()?;

        // // Add the source to the measurement pipeline
        // let _ = alumet.add_source("counter", Box::new(source_energy), trigger_s);

        // === Transform ===

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


// struct ExampleSource {
//     metric_energy: TypedMetricId<f64>,
//      metric_energy_prefixed: TypedMetricId<f64>,
//     metric_not_energy: TypedMetricId<u64>,
// }
// // For testing only
// impl Source for ExampleSource {
//     fn poll(&mut self, acc: &mut MeasurementAccumulator, timestamp: Timestamp) -> std::result::Result<(), PollError> {
//         log::info!("Poll !!");

//         let point_energy = MeasurementPoint::new(
//             timestamp,
//             self.metric_energy,
//             Resource::LocalMachine,
//             ResourceConsumer::LocalMachine,
//             42.123,  // Measured value
//         );

//         let point_energy_prefixed = MeasurementPoint::new(
//             timestamp,
//             self.metric_energy_prefixed,
//             Resource::LocalMachine,
//             ResourceConsumer::LocalMachine,
//             31415.0,  // Measured value
//         );

//         let point_not_energy = MeasurementPoint::new(
//             timestamp,
//             self.metric_not_energy,
//             Resource::LocalMachine,
//             ResourceConsumer::LocalMachine,
//             2,  // Measured value
//         );
//         acc.push(point_energy);
//         acc.push(point_energy_prefixed);
//         acc.push(point_not_energy);
//         Ok(())
//     }
// }

// === Transform bellow ===

struct EnergyToCarbonTransform {
    carbon_emission: RawMetricId,
    emission_intensity_provider: Box<dyn EmissionIntensityProvider>,
}

impl Transform for EnergyToCarbonTransform {
    fn apply(&mut self, measurements: &mut MeasurementBuffer, _ctx: &TransformContext) -> std::result::Result<(), TransformError> {
        // self.emission_intensity_provider.get_intensity().unwrap()
        let mut carbon_points = Vec::new();

        for m in measurements.iter() {
            let metric = _ctx.metrics.by_id(&m.metric).unwrap();
            // If the metric is in <prefix>joules => convert to joules => transform to gCo2 => add it to `carbon_points`

           let mut factor: f64 = 0.0; // 0.0 means "not a joule unit"
            match &metric.unit {
                u if *u == PrefixedUnit::nano(Unit::Joule)   => factor = 1e-9,
                u if *u == PrefixedUnit::micro(Unit::Joule)  => factor = 1e-6,
                u if *u == PrefixedUnit::milli(Unit::Joule)  => factor = 1e-3,
                u if *u == PrefixedUnit::from(Unit::Joule)   => factor = 1.0,
                u if *u == PrefixedUnit::kilo(Unit::Joule)   => factor = 1e3,
                u if *u == PrefixedUnit::mega(Unit::Joule)   => factor = 1e6,
                u if *u == PrefixedUnit::giga(Unit::Joule)   => factor = 1e9,
                _ => {}
            }

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
                    // ! need to call get_intensity() at every apply, even if the value is fixed
                    WrappedMeasurementValue::F64(energy * factor * self.emission_intensity_provider.get_intensity().unwrap()),
                ));
            } 
        }
        
        for point in carbon_points {
            measurements.push(point);
        }

        Ok(())
    
    }
}