use alumet::{
    metrics::def::MetricId,
    plugin::{
        AlumetPluginStart, ConfigTable,
        rust::{AlumetPlugin, deserialize_config},
    },
    units::Unit,
};
use anyhow::Context;

use crate::formula::{config::FormulaConfig, transform::GenericAttributionTransform};

mod formula;

pub struct EnergyAttributionPlugin {
    config: Option<FormulaConfig>,
}

impl AlumetPlugin for EnergyAttributionPlugin {
    fn name() -> &'static str {
        "energy-attribution"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config)?;
        Ok(Box::new(Self { config: Some(config) }))
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        Ok(None)
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        let formula_config = self.config.take().unwrap();
        let result_metric = alumet.create_metric::<f64>(
            "attributed_energy",
            Unit::Joule,
            "Energy attribution (since the previous value) per consumer and per resource",
        )?;

        // create the transform, in a builder because we need the metric registry
        let _ = alumet.add_transform_builder("attribution_transform", move |ctx| {
            let res = formula::prepare(formula_config, ctx.metrics(), result_metric)
                .context("failed to prepare attribution formula; check that you have enabled the required sources and that the configuration is correct");
            let (formula, params) = res?;
            let transform = Box::new(GenericAttributionTransform::new(formula, params));
            Ok(transform)
        })?;
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}
