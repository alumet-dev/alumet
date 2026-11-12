//! Checks that outputs can read the exact current poll interval of the source
//! that produced each measurement, and that the value follows dynamic reconfigurations.

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use alumet::{
    agent::{self, plugin::PluginSet},
    measurement::{MeasurementAccumulator, MeasurementBuffer, MeasurementPoint, Timestamp},
    metrics::TypedMetricId,
    pipeline::{
        Builder,
        control::{PluginControlHandle, request},
        elements::{
            error::{PollError, WriteError},
            output::{Output, OutputContext},
            source::{Source, trigger},
        },
        matching::SourceNamePattern,
        naming::PluginName,
    },
    plugin::{AlumetPluginStart, ConfigTable, Plugin, PluginMetadata},
    resources::{Resource, ResourceConsumer},
    units::Unit,
};

const HOUR: Duration = Duration::from_secs(3600);
const FAST: Duration = Duration::from_millis(500);
const CONTROL_TIMEOUT: Duration = Duration::from_millis(500);

/// (metric name, poll intervals seen by the output) for each measurement written.
type Observations = Arc<Mutex<Vec<(String, Vec<Duration>)>>>;

fn init_plugin(observations: Observations) -> impl FnOnce(ConfigTable) -> anyhow::Result<Box<dyn Plugin>> {
    move |_| Ok(Box::new(TestPlugin { observations }))
}

struct TestPlugin {
    observations: Observations,
}

impl Plugin for TestPlugin {
    fn name(&self) -> &str {
        "test"
    }

    fn version(&self) -> &str {
        "0.0.1"
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        let metric_a = alumet.create_metric::<u64>("metric_a", Unit::Unity, "metric of src_a")?;
        let metric_b = alumet.create_metric::<u64>("metric_b", Unit::Unity, "metric of src_b")?;
        let metric_shared = alumet.create_metric::<u64>("metric_shared", Unit::Unity, "metric of both sources")?;

        // Two sources of the same plugin, with different poll intervals (like procfs kernel/network).
        // Both produce the same 'metric_shared' id (like cpu_time_delta for containers/cgroups).
        alumet.add_source(
            "src_a",
            Box::new(TestSource {
                metrics: vec![metric_a, metric_shared.clone()],
            }),
            slow_trigger(),
        )?;
        alumet.add_source(
            "src_b",
            Box::new(TestSource {
                metrics: vec![metric_b, metric_shared],
            }),
            slow_trigger(),
        )?;

        alumet.add_blocking_output(
            "out",
            Box::new(RecordingOutput {
                observations: self.observations.clone(),
            }),
        )?;
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn pre_pipeline_start(&mut self, _ctx: &mut alumet::plugin::AlumetPreStart) -> anyhow::Result<()> {
        Ok(())
    }

    fn post_pipeline_start(&mut self, _ctx: &mut alumet::plugin::AlumetPostStart) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Never triggers by itself (starting in one hour), only on manual trigger.
fn slow_trigger() -> trigger::TriggerSpec {
    trigger::builder::time_interval(HOUR)
        .starting_at(Instant::now() + HOUR)
        .allow_manual_trigger()
        .build()
        .unwrap()
}

struct TestSource {
    metrics: Vec<TypedMetricId<u64>>,
}

impl Source for TestSource {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        for metric in &self.metrics {
            measurements.push(MeasurementPoint::new(
                timestamp,
                metric.clone(),
                Resource::LocalMachine,
                ResourceConsumer::LocalMachine,
                1u64,
            ));
        }
        Ok(())
    }
}

struct RecordingOutput {
    observations: Observations,
}

impl Output for RecordingOutput {
    fn write(&mut self, measurements: &MeasurementBuffer, ctx: &OutputContext) -> Result<(), WriteError> {
        let mut obs = self.observations.lock().unwrap();
        for m in measurements {
            let name = ctx
                .metrics
                .by_id(&m.metric)
                .ok_or_else(|| WriteError::Fatal(anyhow::anyhow!("unknown metric")))?
                .name
                .clone();
            let intervals = ctx.poll_intervals_of(measurements, &m.metric);
            obs.push((name, intervals));
        }
        Ok(())
    }
}

#[test]
fn output_sees_exact_poll_interval_of_each_source() {
    let observations: Observations = Arc::new(Mutex::new(Vec::new()));
    let plugins = PluginSet::from(vec![PluginMetadata {
        name: "test".to_owned(),
        version: "0.0.1".to_owned(),
        init: Box::new(init_plugin(observations.clone())),
        default_config: Box::new(|| Ok(None)),
    }]);

    let agent = agent::Builder::from_pipeline(plugins, Builder::new())
        .build_and_start()
        .expect("agent should start");
    let control = &agent
        .pipeline
        .control_handle()
        .with_plugin(PluginName("test".to_owned()));
    agent
        .pipeline
        .async_runtime()
        .block_on(scenario(control))
        .expect("scenario failed");
    control.shutdown();
    agent.wait_for_shutdown(CONTROL_TIMEOUT).unwrap();

    let obs = observations.lock().unwrap();
    // first round: both sources still poll at HOUR (per-source, exact)
    // second round: src_a has been reconfigured to FAST, src_b is untouched.
    let expected = [
        ("metric_a".to_string(), vec![HOUR]),
        ("metric_shared".to_string(), vec![HOUR]),
        ("metric_b".to_string(), vec![HOUR]),
        ("metric_shared".to_string(), vec![HOUR]),
        ("metric_a".to_string(), vec![FAST]),
        ("metric_shared".to_string(), vec![FAST]),
        ("metric_b".to_string(), vec![HOUR]),
        ("metric_shared".to_string(), vec![HOUR]),
    ];
    assert_eq!(obs.len(), expected.len(), "observations: {obs:?}");
    for (observed, expected) in obs.iter().zip(&expected) {
        assert_eq!(observed, expected);
    }
}

async fn scenario(control: &PluginControlHandle) -> anyhow::Result<()> {
    // Round 1: each source produces metric_x and metric_shared, both poll at HOUR.
    poll_now(control, "src_a").await?;
    poll_now(control, "src_b").await?;

    // Reconfigure only src_a: set-period 500ms (as socket-control does).
    let new_trigger = trigger::builder::time_interval(FAST)
        .starting_at(Instant::now() + HOUR)
        .allow_manual_trigger()
        .update_interval(Duration::from_millis(100))
        .build()?;
    control
        .send_wait(
            request::source(SourceNamePattern::exact("test", "src_a")).set_trigger(new_trigger),
            CONTROL_TIMEOUT,
        )
        .await?;
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Round 2: measurements of src_a now report FAST, measurements of src_b still HOUR.
    poll_now(control, "src_a").await?;
    poll_now(control, "src_b").await?;
    Ok(())
}

async fn poll_now(control: &PluginControlHandle, source: &str) -> anyhow::Result<()> {
    control
        .send_wait(
            request::source(SourceNamePattern::exact("test", source)).trigger_now(),
            CONTROL_TIMEOUT,
        )
        .await?;
    tokio::time::sleep(Duration::from_millis(200)).await;
    Ok(())
}
