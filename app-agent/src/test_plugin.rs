use alumet::{metrics::{MeasurementAccumulator, MeasurementBuffer, MeasurementPoint, MeasurementValue, MetricId, MeasurementType, ResourceId}, pipeline::{Output, PollError, Source, Transform, TransformError, WriteError}, plugin::{AlumetStart, Plugin, PluginError}, units::Unit};


pub struct TestPlugin;
struct TestSource {
    metric_a: MetricId,
    metric_b: MetricId,
    b_counter: u64,
}
struct TestTransform;
struct TestOutput;

impl TestPlugin {
    pub fn init() -> Box<TestPlugin> {
        Box::new(TestPlugin)
    }
}
impl Plugin for TestPlugin {
    fn name(&self) -> &str {
        "test-plugin"
    }

    fn version(&self) -> &str {
        "0.0.1"
    }

    #[rustfmt::skip] 
    fn start(&mut self, alumet: &mut AlumetStart) -> Result<(), PluginError> {
        // Register the metrics
        let metric_a = alumet.create_metric("test-energy-a", MeasurementType::UInt, Unit::Watt, "Test metric A, in Watts.")?;
        let metric_b = alumet.create_metric("test-counter-b", MeasurementType::UInt, Unit::Unity, "Test metric B, counter without unit.")?;
        
        // Add steps to the pipeline
        alumet.add_source(Box::new(TestSource{metric_a,metric_b,b_counter:0}));
        alumet.add_transform(Box::new(TestTransform));
        alumet.add_output(Box::new(TestOutput));
        Ok(())
    }

    fn stop(&mut self) -> Result<(), PluginError> {
        todo!()
    }
}

impl Source for TestSource {
    fn poll(&mut self, acc: &mut MeasurementAccumulator, timestamp: std::time::SystemTime) -> Result<(), PollError> {
        // generate some values for testing purposes, that evolve over time
        self.b_counter += 1;
        let value_a = 98 + 4*(self.b_counter % 2);
        
        // create a "resource id" to tag the data with an information about what the measurement is about
        let resource = ResourceId::custom("test", "imaginary-thing");

        // push the values to ALUMET pipeline
        acc.push(MeasurementPoint::new(timestamp, self.metric_a, resource.clone(), MeasurementValue::UInt(value_a)));
        acc.push(MeasurementPoint::new(timestamp, self.metric_b, resource, MeasurementValue::UInt(self.b_counter)));

        Ok(())
    }
}

impl Transform for TestTransform {
    fn apply(&mut self, measurements: &mut MeasurementBuffer) -> Result<(), TransformError> {
        fn copy_and_change_to_float(m: &MeasurementPoint) -> MeasurementPoint {
            let mut res = m.clone();
            res.value = match res.value {
                f @ MeasurementValue::Float(_) => f,
                MeasurementValue::UInt(i) => MeasurementValue::Float(i as f64),
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
    fn write(&mut self, measurements: &MeasurementBuffer) -> Result<(), WriteError> {
        for m in measurements.iter() {
            let ts = &m.timestamp;
            let res_kind = m.resource.kind();
            let res_id = m.resource.id_str();
            let name = m.metric.name();
            let value = &m.value;
            println!(">> {ts:?} on {res_kind} {res_id} :{name} = {value:?}");
        }
        Ok(())
    }
}
