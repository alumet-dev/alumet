use std::cell::LazyCell;

use alumet::{
    measurement::{AttributeValue, MeasurementBuffer, MeasurementPoint},
    pipeline::{
        Transform,
        elements::{error::TransformError, transform::TransformContext},
    },
};

use crate::job_tracker::JobTracker;

/// Add the list of current jobs to every measurement that is not job-specific.
/// This is used to relate the measurements to the jobs, for searching, making dashboards, etc.
pub struct JobInfoAttacher {
    tracker: JobTracker,
}

impl JobInfoAttacher {
    pub fn new(tracker: JobTracker) -> Self {
        Self { tracker }
    }
}

fn attach_involved_jobs(m: &mut MeasurementPoint, jobs: &Vec<u64>) {
    let jobs_attr = jobs.clone();
    m.add_attr("involved_jobs", AttributeValue::ListU64(jobs_attr));
}

impl Transform for JobInfoAttacher {
    fn apply(&mut self, measurements: &mut MeasurementBuffer, _ctx: &TransformContext) -> Result<(), TransformError> {
        // lazily initialized
        let current_job_list = LazyCell::new(|| self.tracker.known_jobs_sorted().into_iter().collect::<Vec<_>>());
        for m in measurements.iter_mut() {
            if !m.attributes_keys().any(|k| k == "job_id") {
                // This measurement is not job-specific, attach the list of running jobs.
                // See issue #209.
                attach_involved_jobs(m, &current_job_list);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    use alumet::{
        agent::{
            self,
            plugin::{PluginInfo, PluginSet},
        },
        measurement::{MeasurementAccumulator, MeasurementPoint, Timestamp, WrappedMeasurementValue},
        metrics::{TypedMetricId, def::RawMetricId},
        pipeline::{
            Source,
            elements::{error::PollError, source::trigger::TriggerSpec},
            naming::TransformName,
        },
        plugin::{AlumetPluginStart, ConfigTable, PluginMetadata, rust::AlumetPlugin},
        resources::{Resource, ResourceConsumer},
        test::{
            RuntimeExpectations,
            runtime::{TransformCheckInputContext, TransformCheckOutputContext},
        },
        units::Unit,
    };

    const PLUGIN_NAME: &str = "test-plugin";
    const SOURCE_NAME: &str = "test-source";
    const TRANSFORM_NAME: &str = "test-job";

    struct MockSource {
        metric: TypedMetricId<u64>,
    }

    impl Source for MockSource {
        fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
            measurements.push(MeasurementPoint::new(
                timestamp,
                self.metric,
                Resource::LocalMachine,
                ResourceConsumer::LocalMachine,
                1,
            ));
            Ok(())
        }
    }

    struct MockPlugin;

    impl AlumetPlugin for MockPlugin {
        fn name() -> &'static str {
            PLUGIN_NAME
        }

        fn version() -> &'static str {
            "0.1.0"
        }

        fn default_config() -> anyhow::Result<Option<ConfigTable>> {
            Ok(None)
        }

        fn init(_config: ConfigTable) -> anyhow::Result<Box<Self>> {
            Ok(Box::new(Self))
        }

        fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
            let metric = alumet.create_metric("metric", Unit::Unity, "test metric")?;

            let mut tracker = JobTracker::new();
            tracker.add_multiple(vec![1, 2, 3].into_iter());

            alumet.add_transform(TRANSFORM_NAME, Box::new(JobInfoAttacher::new(tracker)))?;

            alumet.add_source(
                SOURCE_NAME,
                Box::new(MockSource { metric }),
                TriggerSpec::at_interval(Duration::from_secs(1)),
            )?;

            Ok(())
        }

        fn stop(&mut self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn test_apply_if_job_id() {
        let mut plugins = PluginSet::new();

        plugins.add_plugin(PluginInfo {
            metadata: PluginMetadata::from_static::<MockPlugin>(),
            enabled: true,
            config: None,
        });

        let make_input = |_ctx: &mut TransformCheckInputContext| {
            let mut buffer = MeasurementBuffer::new();

            let mut point = MeasurementPoint::new_untyped(
                Timestamp::now(),
                RawMetricId::from_u64(0),
                Resource::LocalMachine,
                ResourceConsumer::LocalMachine,
                WrappedMeasurementValue::U64(1),
            );

            point.add_attr("job_id", 2);

            buffer.push(point);
            buffer
        };

        let check_output = |ctx: &mut TransformCheckOutputContext| {
            let measurements = ctx.measurements();
            let mut iter = measurements.iter();
            let m = iter.next().expect("measurement expected");

            assert!(iter.next().is_none());
            assert!(m.attributes_keys().any(|k| k == "job_id"));
            assert!(!m.attributes_keys().any(|k| k == "involved_jobs"));
        };

        let expectations = RuntimeExpectations::new().test_transform(
            TransformName::from_str(PLUGIN_NAME, TRANSFORM_NAME),
            make_input,
            check_output,
        );

        let agent = agent::Builder::new(plugins)
            .with_expectations(expectations)
            .build_and_start()
            .unwrap();

        agent.wait_for_shutdown(Duration::from_secs(2)).unwrap();
    }

    #[test]
    fn test_attach_involved_jobs() {
        let mut point = MeasurementPoint::new_untyped(
            Timestamp::now(),
            RawMetricId::from_u64(0),
            Resource::LocalMachine,
            ResourceConsumer::LocalMachine,
            WrappedMeasurementValue::U64(1),
        );

        let jobs = vec![1, 2, 3];
        attach_involved_jobs(&mut point, &jobs);

        assert!(point.attributes_keys().any(|k| k == "involved_jobs"));
        match point.attributes().find(|(k, _)| *k == "involved_jobs").unwrap().1 {
            AttributeValue::ListU64(v) => {
                assert_eq!(v.as_slice(), &jobs);
            }
            _ => panic!("wrong type"),
        }
    }
}
