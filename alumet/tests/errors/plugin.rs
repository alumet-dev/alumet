use std::time::Duration;

use anyhow::Context;

use super::points::{error_point, panic_point};

use alumet::measurement::{MeasurementAccumulator, MeasurementBuffer, Timestamp};
use alumet::pipeline::elements::error::{PollError, TransformError, WriteError};
use alumet::pipeline::elements::output::OutputContext;
use alumet::pipeline::elements::source::builder::ManagedSource;
use alumet::pipeline::elements::transform::TransformContext;
use alumet::pipeline::trigger::TriggerSpec;
use alumet::pipeline::{Output, Source, Transform};
use alumet::plugin::{
    rust::{serialize_config, AlumetPlugin},
    AlumetPluginStart, AlumetPostStart, ConfigTable,
};

pub struct BadPlugin;
pub struct BadSource1;
pub struct BadSource2;
pub struct BadTransform;
pub struct BadOutput;

impl AlumetPlugin for BadPlugin {
    fn name() -> &'static str {
        panic_point!(name);
        "errors"
    }

    fn version() -> &'static str {
        panic_point!(version);
        "0.0.1"
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        error_point!(default_config);
        let config = serialize_config(Duration::from_secs(1))?;
        Ok(Some(config))
    }

    fn init(_config: ConfigTable) -> anyhow::Result<Box<Self>> {
        error_point!(init);
        Ok(Box::new(BadPlugin))
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        error_point!(start);
        alumet
            .add_source_builder("source1", |_| {
                error_point!(source1_build);
                Ok(ManagedSource {
                    trigger_spec: TriggerSpec::at_interval(Duration::from_millis(100)),
                    source: Box::new(BadSource1),
                })
            })
            .expect("name 'source1' should be unique among sources");
        alumet
            .add_transform_builder("transform", |_| {
                error_point!(transf_build);
                Ok(Box::new(BadTransform))
            })
            .expect("name 'transform' should be unique among transforms");
        alumet
            .add_blocking_output_builder("output", |_| {
                error_point!(output_build);
                Ok(Box::new(BadOutput))
            })
            .expect("name 'output' should be unique among outputs");
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        error_point!(stop);
        Ok(())
    }

    fn post_pipeline_start(&mut self, alumet: &mut AlumetPostStart) -> anyhow::Result<()> {
        error_point!(post_pipeline_start);
        let control_handle = alumet.pipeline_control();
        control_handle
            .add_source_builder("source2", |_| {
                error_point!(source2_build);
                Ok(ManagedSource {
                    trigger_spec: TriggerSpec::at_interval(Duration::from_millis(100)),
                    source: Box::new(BadSource2),
                })
            })
            .context("failed to add source in post_pipeline_start")?;
        Ok(())
    }
}

impl Drop for BadPlugin {
    fn drop(&mut self) {
        panic_point!(drop);
    }
}

impl Source for BadSource1 {
    fn poll(&mut self, _m: &mut MeasurementAccumulator, _t: Timestamp) -> Result<(), PollError> {
        error_point!(source1_poll);
        Ok(())
    }
}

impl Source for BadSource2 {
    fn poll(&mut self, _m: &mut MeasurementAccumulator, _t: Timestamp) -> Result<(), PollError> {
        error_point!(source2_poll);
        Ok(())
    }
}

impl Transform for BadTransform {
    fn apply(&mut self, _m: &mut MeasurementBuffer, _ctx: &TransformContext) -> Result<(), TransformError> {
        error_point!(transf_apply);
        Ok(())
    }
}

impl Output for BadOutput {
    fn write(&mut self, _m: &MeasurementBuffer, _ctx: &OutputContext) -> Result<(), WriteError> {
        error_point!(output_write);
        Ok(())
    }
}
