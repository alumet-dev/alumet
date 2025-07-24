use alumet::{
    metrics::TypedMetricId,
    // pipeline::{
    //     control::{matching::SourceMatcher, request},
    //     elements::source::trigger::builder::ManualTriggerBuilder,
    //     naming::SourceName,
    // },
    plugin::{
        AlumetPluginStart,
        ConfigTable,
        // event::{self},
        rust::{AlumetPlugin, deserialize_config, serialize_config},
    },
    units::Unit,
};
use serde::{Deserialize, Serialize};
// use serde_json::Value;
// use std::error::Error;
use std::sync::{Arc, Mutex};
// use std::time::{Duration, SystemTime};
// use time::OffsetDateTime;
// use tokio::task;

/// Structure for Quarch implementation
pub struct QuarchPlugin {
    config: Arc<Mutex<ParsedConfig>>,
}

///   Implementation of Quarch plugin as an alumet plugin
impl AlumetPlugin for QuarchPlugin {
    fn name() -> &'static str {
        "quarch"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        Ok(Some(serialize_config(Config::default())?))
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config: Config = deserialize_config(config)?;
        let parsed_config = ParsedConfig {
            metrics: config.metrics,
            metric_ids: Vec::new(),
        };
        Ok(Box::new(QuarchPlugin {
            config: Arc::new(Mutex::new(parsed_config)),
        }))
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        log::info!("Quarch plugin is starting");
        //let mut use_quarch = true;
        //let mut check_consistency = true;
        //if ...
        //check_consistency = false; ...

        // Create a metric
        let mut config = self.config.lock().unwrap();
        let mut metric_ids = Vec::with_capacity(config.metrics.len());

        for metric_name in &config.metrics {
            let quarch_metric = alumet.create_metric::<f64>(
                metric_name,
                Unit::Watt,
                format!("Disk power consumption for {}", metric_name),
            )?;
            metric_ids.push(quarch_metric);
        }

        config.metric_ids = metric_ids;

        // Discover RAPL domains available in perf_events and powercap. Beware, this can fail!
        // let try_quarch = disk::all_power_events();
        // let (available_domains, subset_indicator) = match (try_quarch) {
        //     (Ok(perf_events), Ok(power_zones)) => {
        //         if !check_consistency {
        //             (SafeSubset::from_powercap_only(power_zones), " (from powercap)")
        //         } else {
        //             let mut safe_domains = check_domains_consistency(&perf_events, &power_zones);
        //             let mut domain_origin = "";
        //             if !safe_domains.is_whole {
        //                 // If one of the domain set is smaller, it could be empty, which would prevent the plugin from measuring anything.
        //                 // In that case, we fall back to the other interface, the one that reports a non-empty list of domains.
        //                 if perf_events.is_empty() && !power_zones.top.is_empty() {
        //                     log::warn!("perf_events returned an empty list of RAPL domains, I will disable perf_events and use powercap instead.");
        //                     use_perf = false;
        //                     safe_domains = SafeSubset::from_powercap_only(power_zones);
        //                     domain_origin = " (from powercap)";
        //                 } else if !perf_events.is_empty() && power_zones.top.is_empty() {
        //                     log::warn!("perf_events returned an empty list of RAPL domains, I will disable powercap and use perf_events instead.");
        //                     use_powercap = false;
        //                     safe_domains = SafeSubset::from_perf_only(perf_events);
        //                     domain_origin = " (from perf_events)";
        //                 } else {
        //                     domain_origin = " (\"safe subset\")";
        //                 }
        //             }
        //             (safe_domains, domain_origin)
        //         }
        //     }
        //     (Ok(perf_events), Err(powercap_err)) => {
        //         log::error!(
        //             "Cannot read the list of RAPL domains available via the powercap interface: {powercap_err:?}."
        //         );
        //         log::warn!("The consistency of the RAPL domains reported by the different interfaces of the Linux kernel cannot be checked (this is useful to work around bugs in some kernel versions on some machines).");
        //         (SafeSubset::from_perf_only(perf_events), " (from perf_events)")
        //     }
        //     (Err(perf_err), Ok(power_zones)) => {
        //         log::warn!(
        //             "Cannot read the list of RAPL domains available via the perf_events interface: {perf_err:?}."
        //         );
        //         log::warn!("The consistency of the RAPL domains reported by the different interfaces of the Linux kernel cannot be checked (this is useful to work around bugs in some kernel versions on some machines).");
        //         (SafeSubset::from_powercap_only(power_zones), " (from powercap)")
        //     }
        //     (Err(perf_err), Err(power_err)) => {
        //         log::error!("I could use neither perf_events nor powercap.\nperf_events error: {perf_err:?}\npowercap error: {power_err:?}");
        //         Err(anyhow!(
        //             "Both perf_events and powercap failed, unable to read RAPL counters: {perf_err}\n{power_err}"
        //         ))?
        //     }
        // };

        // Create the measurement source.
        // let source = match (use_quarch) {
        //     (true) => {
        //         setup_quarch_probe(metric, &available)?
        //     }
        //     (false) => {
        //         return Err(anyhow!(
        //             "I canno't use nquarch: impossible to measure disk power consumption."
        //         ));
        //     }
        // };

        // Configure the source and add it to Alumet
        // let trigger = trigger::builder::time_interval(self.config.poll_interval)
        //     .flush_interval(self.config.flush_interval)
        //     .update_interval(self.config.flush_interval)
        //     .build()
        //     .unwrap();
        // alumet.add_source("in", source, trigger)?;

        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        log::debug!("Quarch plugin is ending!");
        Ok(())
    }
}

// fn setup_quarch_probe(
//     metric: alumet::metrics::TypedMetricId<f64>,
//     available: &SafeSubset,
// ) -> anyhow::Result<Box<dyn Source>> {
//     match PowercapProbe::new(metric, &available.power_zones) {
//         Ok(quarch_probe) => Ok(Box::new(quarch_probe)),
//         Err(e) => {
//             let msg = indoc! {"
//                 I could not use the powercap sysfs to read RAPL energy counters.
//                 This is probably caused by insufficient privileges.
//                 Please check that you have read access to everything in '/sys/devices/virtual/powercap/intel-rapl'.

//                 A solution could be:
//                     sudo chmod a+r -R /sys/devices/virtual/powercap/intel-rapl
//             "};
//             log::error!("{msg}");
//             Err(e)
//         }
//     }
// }

/// A structure that stocks the configuration parameters that are necessary to ...
#[derive(Serialize, Deserialize, Clone)]
struct Config {
    pub metrics: Vec<String>,
    // #[serde(with = "humantime_serde")]
    // poll_interval: Duration,
    // #[serde(with = "humantime_serde")]
    // flush_interval: Duration,
}

struct ParsedConfig {
    metrics: Vec<String>,
    metric_ids: Vec<TypedMetricId<f64>>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            // poll_interval: Duration::from_secs(1),
            // flush_interval: Duration::from_secs(5),
            metrics: vec!["randwrite".to_string()],
        }
    }
}
