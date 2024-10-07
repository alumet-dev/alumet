use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint, Timestamp},
    metrics::{RawMetricId,TypedMetricId},
    pipeline::{elements::error::PollError, trigger::TriggerSpec, Source},
    plugin::rust::{deserialize_config, serialize_config, AlumetPlugin},
    plugin::{AlumetPreStart,AlumetPluginStart, AlumetPostStart, ConfigTable},
    resources::{Resource, ResourceConsumer},
    units::{PrefixedUnit, Unit, UnitPrefix},
};
use serde::{Deserialize, Serialize};
use std::{fs::File, io::Read, time::Duration};
use log::{info, debug};
use std::sync::{Arc, Mutex};

use transform::EnergyEstimationTdpTransform;

mod transform;


// #[derive(Serialize, Deserialize, Debug)]
// struct Config {
//     #[serde(with = "humantime_serde")]
//     poll_interval: Duration,
//     tdp: u32,
// }

pub struct EnergyEstimationTdpPlugin {
    metrics: Arc<Mutex<Metrics>>,
}
#[derive(Default)]
struct Metrics {
    // To attribute the CPU consumption to K8S pods, we need 2 metrics:
    // - cpu usage per pod
    // - energy attribution (to store the result)

    // The other parameters (tdp and number of virtual cpu is provided by configuration)
    cpu_usage_per_pod: Option<RawMetricId>,
    pod_estimate_attributed_energy: Option<TypedMetricId<f64>>,
}

// pub struct EnergyEstimationTdpPlugin {
//     config: Config,
// }

// impl Default for Config {
//     fn default() -> Self {
//         Self {
//             poll_interval: Duration::from_secs(1),
//             tdp: 10,
//         }
//     }
    
// }

// #[derive(Debug)]
// struct EnergyEstimationTdpPluginSource {
//     byte_metric: TypedMetricId<u64>,
// }

impl AlumetPlugin for EnergyEstimationTdpPlugin {
    // So we define the name of the plugin.
    fn name() -> &'static str {
        "EnergyEstimationTdpPlugin"
    }

    // We also define it's version.
    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    // We use the default config by default and on initialization.
    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        Ok(Some(serialize_config(Config::default())?))
    }

    // // We also use the default config on initialization and we deserialize the config
    // // to take in count if there is a different config than the default one.
    // fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
    //     let config = deserialize_config(config)?;
    //     Ok(Box::new(EnergyEstimationTdpPlugin {
    //         config,
    //     }))

    // }

    fn init(_: alumet::plugin::ConfigTable) -> anyhow::Result<Box<Self>> {
        Ok(Box::new(EnergyEstimationTdpPlugin {
            metrics: Arc::new(Mutex::new(Metrics::default())),
        }))
    }


    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        let mut metrics = self.metrics.lock().unwrap();

        // Create the energy attribution metric and add its id to the
        // transform plugin metrics' list.
        metrics.pod_estimate_attributed_energy = Some(alumet.create_metric(
            "estimate_attributed_energy",
            Unit::Joule,
            "Energy consumption estimated to the pod",
        )?);

        // Add the transform now but fill its metrics later.
        alumet.add_transform(Box::new(EnergyEstimationTdpTransform::new(self.metrics.clone())));
        Ok(())
    }

    // // The start function is here to register metrics, sources and output.
    // fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
    //     let byte_metric =
    //         alumet.create_metric::<u64>("random_byte", Unit::Byte, "A random number")?;
    //     // We create a source from ThePluginSource structure.
    //     let initial_source = Box::new(EnergyEstimationTdpPluginSource {
    //         byte_metric
    //     });

    //     log::trace!("EZC: read tdp value: {}", self.config.tdp);

    //     // Then we add it to the alumet sources, adding the poll_interval value previously defined in the config.
    //     alumet.add_source(
    //         initial_source,
    //         TriggerSpec::at_interval(self.config.poll_interval),
    //     );

    //     Ok(())
        
    // }

    fn pre_pipeline_start(&mut self, alumet: &mut AlumetPreStart) -> anyhow::Result<()> {
        /// Finds the RawMetricId with the name of the metric.
        /// Will only run once, just before the pipeline starts.
        fn find_metric_by_name(alumet: &mut AlumetPreStart, name: &str) -> anyhow::Result<RawMetricId> {
            let (id, _metric) = alumet
                .metrics()
                .into_iter()
                .find(|m| m.1.name == name)
                .expect(&format!("Cannot find metric {name}, is the 'rapl' plugin loaded?").to_string());
            Ok(id.to_owned())
        }

        // Lock the metrics mutex to apply its modifications.
        let mut metrics = self.metrics.lock().unwrap();

        //metrics.cpu_usage_per_pod = Some(find_metric_by_name(alumet, "cpu_usage_per_pod")?);
        Ok(())
    }

    // The stop function is called after all the metrics, sources and output previously
    // registered have been stopped and unregistered.
    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}


#[derive(Deserialize, Serialize)]
struct Config {
    oui: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            oui: String::from("oui"),
        }
    }
}

// impl Source for EnergyEstimationTdpPluginSource {
//     fn poll(
//         &mut self,
//         measurements: &mut MeasurementAccumulator,
//         timestamp: Timestamp,
//     ) -> Result<(), PollError> {
//         let mut rng = File::open("/dev/urandom")?; // Open the "/dev/urandom" file to obtain random data

//         let mut buffer = [0u8; 8]; // Create a mutable buffer of type [u8; 8] (an array of 8 unsigned 8-bit integer)
//         rng.read_exact(&mut buffer)?; // Read enough byte from the file and store the value in the buffer
//         let value = u64::from_le_bytes(buffer);

//         //let tdpLocal: u32= self.config.tdp;

//         log::debug!("EZC: enter in poll");
//         let measurement = MeasurementPoint::new(
//             timestamp,
//             self.byte_metric,
//             Resource::LocalMachine,
//             ResourceConsumer::LocalMachine,
//             value,
//         )
//         .with_attr("double", value.div_euclid(2));

//         log::trace!("EZC: metric value: {} ", measurement.metric.as_u64());
//         //measurements.push(measurement );
        

//         Ok(())
//     }
// }
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
