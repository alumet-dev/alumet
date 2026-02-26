use alumet::plugin::{
    ConfigTable,
    rust::{AlumetPlugin, deserialize_config, serialize_config},
};
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use transform::FilterTransform;

mod transform;

pub struct FilterPlugin {
    config: Config,
}

impl AlumetPlugin for FilterPlugin {
    fn name() -> &'static str {
        "filter"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        Ok(Some(serialize_config(Config::default())?))
    }

    fn init(config: alumet::plugin::ConfigTable) -> anyhow::Result<Box<Self>> {
        let config: Config = deserialize_config(config)?;
        if config.include.is_some() && config.exclude.is_some() {
            return Err(anyhow::anyhow!(
                "filter transform cannot have both include and exclude configuration parameters defined"
            ));
        }
        Ok(Box::new(FilterPlugin { config }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        let include = self.config.include.clone();
        let exclude = self.config.exclude.clone();

        if include.is_none() && exclude.is_none() {
            log::warn!(
                "filter plugin was started but as there's neither 'include' or 'exclude' configuration set, this will do nothing"
            );
            return Ok(());
        }

        alumet.add_transform_builder("transform", move |ctx| {
            let include_metrics_ids = if let Some(metric_names) = &include {
                let mut ids = HashSet::new();
                for metric_name in metric_names {
                    ids.insert(
                        ctx.metric_by_name(metric_name)
                            .with_context(|| format!("Metric not found : {}", metric_name))?
                            .0,
                    );
                }
                Some(ids)
            } else {
                None
            };

            let exclude_metrics_ids = if let Some(metric_names) = &exclude {
                let mut ids = HashSet::new();
                for metric_name in metric_names {
                    ids.insert(
                        ctx.metric_by_name(metric_name)
                            .with_context(|| format!("Metric not found : {}", metric_name))?
                            .0,
                    );
                }
                Some(ids)
            } else {
                None
            };

            let transform = Box::new(FilterTransform::new(include_metrics_ids, exclude_metrics_ids)?);
            Ok(transform)
        })?;
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[derive(Default, Deserialize, Serialize)]
pub struct Config {
    include: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
}
