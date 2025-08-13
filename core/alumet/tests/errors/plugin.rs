use std::time::Duration;

use alumet::pipeline::control::request;
use anyhow::Context;

use super::points::{error_point, panic_point};

use alumet::measurement::{MeasurementAccumulator, MeasurementBuffer, Timestamp};
use alumet::pipeline::elements::error::PollError;
use alumet::pipeline::elements::output::{OutputContext, WriteError};
use alumet::pipeline::elements::source::control::TaskState;
use alumet::pipeline::elements::source::{builder::ManagedSource, trigger::TriggerSpec};
use alumet::pipeline::elements::transform::{TransformContext, TransformError};
use alumet::pipeline::{Output, Source, Transform};
use alumet::plugin::{
    AlumetPluginStart, AlumetPostStart, ConfigTable,
    rust::{AlumetPlugin, serialize_config},
};

pub struct BadPlugin;
pub struct BadSource1;
pub struct BadSource2;
pub struct BadSource3;
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
                    initial_state: TaskState::Run,
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
        let create_source2 = request::create_one().add_source_builder("source2", |_| {
            error_point!(source2_build);
            Ok(ManagedSource {
                initial_state: TaskState::Run,
                trigger_spec: TriggerSpec::at_interval(Duration::from_millis(100)),
                source: Box::new(BadSource2),
            })
        });
        let create_source3 = request::create_one().add_source_builder("source3", |_| {
            error_point!(source3_build);
            Ok(ManagedSource {
                initial_state: TaskState::Run,
                trigger_spec: TriggerSpec::at_interval(Duration::from_millis(100)),
                source: Box::new(BadSource3),
            })
        });
        // create source2, don't catch errors here
        alumet
            .async_runtime()
            .block_on(control_handle.dispatch(create_source2, Duration::from_secs(1)))
            .context("failed to add source2 in post_pipeline_start")?;
        // create source3, wait for the result and catch errors here (build error => post_pipeline_start error)
        alumet
            .async_runtime()
            .block_on(control_handle.send_wait(create_source3, Duration::from_secs(1)))
            .context("failed to add source3 in post_pipeline_start")?;
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

impl Source for BadSource3 {
    fn poll(&mut self, _m: &mut MeasurementAccumulator, _t: Timestamp) -> Result<(), PollError> {
        error_point!(source3_poll);
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
