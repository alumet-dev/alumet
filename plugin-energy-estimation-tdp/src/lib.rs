use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint, Timestamp},
    metrics::TypedMetricId,
    pipeline::{elements::error::PollError, trigger::TriggerSpec, Source},
    plugin::{
        rust::{deserialize_config, serialize_config, AlumetPlugin},
        AlumetPluginStart, AlumetPostStart, ConfigTable,
    },
    resources::{Resource, ResourceConsumer},
    units::{PrefixedUnit, Unit, UnitPrefix},
};
use serde::{Deserialize, Serialize};
use std::{fs::File, io::Read, time::Duration};

#[derive(Serialize, Deserialize, Debug)]
struct Config {
    #[serde(with = "humantime_serde")]
    poll_interval: Duration,
}

pub struct MyPlugin {
    config: Config,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(1),
        }
    }
}

#[derive(Debug)]
struct MyPluginSource {
    byte_metric: TypedMetricId<u64>,
}

impl AlumetPlugin for MyPlugin {
    // So we define the name of the plugin.
    fn name() -> &'static str {
        "MyPlugin"
    }

    // We also define it's version.
    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    // We use the default config by default and on initialization.
    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        Ok(Some(serialize_config(Config::default())?))
    }

    // We also use the default config on initialization and we deserialize the config
    // to take in count if there is a different config than the default one.
    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config)?;
        Ok(Box::new(MyPlugin {
            config,
        }))
    }

    // The start function is here to register metrics, sources and output.
    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        let byte_metric =
            alumet.create_metric::<u64>("random_byte", Unit::Byte, "A random number")?;
        // We create a source from ThePluginSource structure.
        let initial_source = Box::new(MyPluginSource {
            byte_metric
        });

        // Then we add it to the alumet sources, adding the poll_interval value previously defined in the config.
        alumet.add_source(
            initial_source,
            TriggerSpec::at_interval(self.config.poll_interval),
        );
        Ok(())
    }
    // The stop function is called after all the metrics, sources and output previously
    // registered have been stopped and unregistered.
    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

impl Source for MyPluginSource {
    fn poll(
        &mut self,
        measurements: &mut MeasurementAccumulator,
        timestamp: Timestamp,
    ) -> Result<(), PollError> {
        let mut rng = File::open("/dev/urandom")?; // Open the "/dev/urandom" file to obtain random data

        let mut buffer = [0u8; 8]; // Create a mutable buffer of type [u8; 8] (an array of 8 unsigned 8-bit integer)
        rng.read_exact(&mut buffer)?; // Read enough byte from the file and store the value in the buffer
        let value = u64::from_le_bytes(buffer);
        let measurement = MeasurementPoint::new(
            timestamp,
            self.byte_metric,
            Resource::LocalMachine,
            ResourceConsumer::LocalMachine,
            value,
        )
        .with_attr("double", value.div_euclid(2));
        measurements.push(measurement );

        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
