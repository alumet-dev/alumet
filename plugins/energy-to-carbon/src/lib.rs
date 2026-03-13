use std::time::Duration;
use alumet::{
    units::PrefixedUnit,
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
    units::Unit,
};
use serde::{Serialize, Deserialize};

pub struct EnergyToCarbonPlugin{
    config: Config,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
struct Config {
    /// Time between each activation of the counter source.
    emission_intensity_override: f64,
    #[serde(with = "humantime_serde")]
    poll_interval: Duration,
    replace_metrics: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            emission_intensity_override: 475.0,
            poll_interval: Duration::from_secs(1),
            replace_metrics: true,
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

        // Test metric
        let energy = alumet.create_metric::<f64> (
            "exemple_energy",
            Unit::Joule,
            "42j sent every seconds (for testing only)",
        )?;

        // Test metric
        let not_energy = alumet.create_metric::<u64> (
            "exemple_not_energy",
            Unit::Second,
            "2s sent every seconds (for testing only)",
        )?;

        // create the sources
        let source_energy = ExampleSource {
            metric_energy: energy,
            metric_not_energy: not_energy,
            config: self.config.clone(),
        };

        // How the source is triggered
        let trigger_s = trigger::builder::time_interval(self.config.poll_interval).build()?;

        // Add the source to the measurement pipeline
        let _ = alumet.add_source("counter", Box::new(source_energy), trigger_s);

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
            config: self.config.clone(),
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

struct ExampleSource {
    metric_energy: TypedMetricId<f64>,
    metric_not_energy: TypedMetricId<u64>,
    config: Config,
}
// For testing only
impl Source for ExampleSource {
    fn poll(&mut self, acc: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        log::info!("Poll !!");

        let point_energy = MeasurementPoint::new(
            timestamp,
            self.metric_energy,
            Resource::LocalMachine,
            ResourceConsumer::LocalMachine,
            42.0,  // Measured value
        );

        let point_not_energy = MeasurementPoint::new(
            timestamp,
            self.metric_not_energy,
            Resource::LocalMachine,
            ResourceConsumer::LocalMachine,
            2,  // Measured value
        );
        acc.push(point_energy);
        acc.push(point_not_energy);
        Ok(())
    }
}

// === Transform bellow ===

struct EnergyToCarbonTransform {
    carbon_emission: RawMetricId,
    config: Config,
}

impl Transform for EnergyToCarbonTransform {
    fn apply(&mut self, measurements: &mut MeasurementBuffer, _ctx: &TransformContext) -> Result<(), TransformError> {

        let mut carbon_points = Vec::new();

        for m in measurements.iter() {
            let metric = _ctx.metrics.by_id(&m.metric).unwrap();
            // If the metric in joules => transform it => add it to `carbon_points`
            if metric.unit == PrefixedUnit::from(Unit::Joule) {
                let energy = match m.value {
                    WrappedMeasurementValue::F64(v) => v,
                    WrappedMeasurementValue::U64(v) => v as f64,
                };

                carbon_points.push(MeasurementPoint::new_untyped(
                    m.timestamp,
                    self.carbon_emission,
                    m.resource.clone(),
                    m.consumer.clone(),
                    WrappedMeasurementValue::F64(energy * self.config.emission_intensity_override),
                ));
            } 
        }
        
        // Remove original joules points, replace with carbon points
        if self.config.replace_metrics {
            let kept: MeasurementBuffer = measurements
            .iter()
            .filter(|m| {
                let metric = _ctx.metrics.by_id(&m.metric).unwrap();
                metric.unit.base_unit != Unit::Joule
            })
            .cloned()
            .collect();
        *measurements = kept;
        }

        for point in carbon_points {
            measurements.push(point);
        }

        Ok(())
    
    }
}