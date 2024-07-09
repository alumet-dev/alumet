use std::time::Duration;

use alumet::measurement::{MeasurementAccumulator, MeasurementBuffer, MeasurementPoint, Timestamp, WrappedMeasurementValue};
use alumet::metrics::TypedMetricId;
use alumet::pipeline::elements::error::{PollError, TransformError, WriteError};
use alumet::pipeline::elements::output::OutputContext;
use alumet::pipeline::elements::transform::TransformContext;
use alumet::pipeline::{trigger, Output, Source, Transform};
use alumet::plugin::{AlumetPostStart, AlumetStart, Plugin};
use alumet::resources::{ResourceConsumer, Resource};
use alumet::units::Unit;

pub struct TestPlugin {
    name: String,
    base_value_a: u64,
    pub state: State,
}
struct TestSource {
    metric_a: TypedMetricId<u64>,
    metric_b: TypedMetricId<u64>,
    a_base: u64,
    b_counter: u64,
}
struct TestTransform;
struct TestOutput;
#[derive(Debug, PartialEq, Eq)]
pub enum State {
    Initialized,
    Started,
    Stopped,
    PostPipelineStart,
}

impl TestPlugin {
    pub fn init(name: &str, base_value_a: u64) -> Box<TestPlugin> {
        Box::new(TestPlugin {
            name: name.to_owned(),
            base_value_a,
            state: State::Initialized,
        })
    }
}
impl Plugin for TestPlugin {
    fn name(&self) -> &str {
        // In the tests, we use multiple instances of TestPlugin with different parameters.
        // In a real-world plugin, you would simply return a &str directly, such as "my-plugin-name".
        &self.name
    }

    fn version(&self) -> &str {
        "0.0.1"
    }

    #[rustfmt::skip]
    fn start(&mut self, alumet: &mut AlumetStart) -> anyhow::Result<()> {
        // Register the metrics (for a normal plugin, you would simply give the name directly as a &str)
        let metric_name_a = self.name.clone() + ":energy-a";
        let metric_name_b = self.name.clone() + ":counter-b";
        let metric_a = alumet.create_metric::<u64>(&metric_name_a, Unit::Watt, "Test metric A, in Watts.")?;
        let metric_b = alumet.create_metric::<u64>(&metric_name_b, Unit::Unity, "Test metric B, counter without unit.")?;

        // Add steps to the pipeline
        let source = Box::new(TestSource{metric_a,metric_b,a_base: self.base_value_a,b_counter:0});
        let trigger = trigger::builder::time_interval(Duration::from_secs(1)).build().unwrap();
        alumet.add_source(source, trigger);
        alumet.add_transform(Box::new(TestTransform));
        alumet.add_output(Box::new(TestOutput));

        // Update state (for testing purposes)
        self.state = State::Started;
        Ok(())
    }

    fn post_pipeline_start(&mut self, _: &mut AlumetPostStart) -> anyhow::Result<()> {
        self.state = State::PostPipelineStart;
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        self.state = State::Stopped;
        Ok(())
    }
}

impl Source for TestSource {
    fn poll(&mut self, acc: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        // generate some values for testing purposes, that evolve over time
        self.b_counter += 1;
        let value_a = self.a_base + 4 * (self.b_counter % 2);

        // create a "resource id" to tag the data with an information about what the measurement is about
        let resource = Resource::custom("test-resource", "imaginary-thing");
        let consumer = ResourceConsumer::custom("test-consumer", "does it work?");

        // push the values to ALUMET pipeline
        acc.push(MeasurementPoint::new(
            timestamp,
            self.metric_a,
            resource.clone(),
            consumer.clone(),
            value_a,
        ));
        acc.push(MeasurementPoint::new(
            timestamp,
            self.metric_b,
            resource,
            consumer.clone(),
            self.b_counter,
        ));

        Ok(())
    }
}

impl Transform for TestTransform {
    fn apply(&mut self, measurements: &mut MeasurementBuffer, ctx: &TransformContext) -> Result<(), TransformError> {
        fn copy_and_change_to_float(m: &MeasurementPoint) -> MeasurementPoint {
            let mut res = m.clone();
            res.value = match res.value {
                f @ WrappedMeasurementValue::F64(_) => f,
                WrappedMeasurementValue::U64(i) => WrappedMeasurementValue::F64(i as f64),
            };
            res
        }
        let copy: Vec<_> = measurements.iter().map(copy_and_change_to_float).collect();
        for m in copy {
            measurements.push(m);
        }
        Ok(())
    }
}

impl Output for TestOutput {
    fn write(&mut self, measurements: &MeasurementBuffer, ctx: &OutputContext) -> Result<(), WriteError> {
        for m in measurements.iter() {
            let ts = &m.timestamp;
            let res_kind = m.resource.kind();
            let res_id = m.resource.id_display();
            let name = ctx.metrics.by_id(&m.metric).unwrap().name.to_owned();
            let value = &m.value;
            println!(">> {ts:?} on {res_kind} {res_id} :{name} = {value:?}");
        }
        Ok(())
    }
}
