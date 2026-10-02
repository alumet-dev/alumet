use std::{
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

use alumet::{
    agent::{self, plugin::PluginSet},
    pipeline::{
        self,
        control::{PluginControlHandle, request},
        elements::source::trigger::{self, TriggerSpec},
        matching::SourceNamePattern,
    },
};
use alumet::{pipeline::naming::PluginName, plugin::PluginMetadata};

mod common;

use common::test_plugin::{AtomicState, MeasurementCounters, State, TestPlugin};

const PLUGIN_NAME: &str = "test";
const SOURCE_NAME: &str = "test";
const CONTROL_TIMEOUT: Duration = Duration::from_millis(500);

fn init_logger() {
    // Ignore errors because the logger can only be initialized once, and we run multiple tests.
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("trace")).try_init();
}

#[test]
fn manual_trigger_should_be_updated_with_trigger_change() -> anyhow::Result<()> {
    init_logger();
    let state = Arc::new(AtomicState::new(State::PreInit));
    let counters = MeasurementCounters::default();

    let initial_trigger = trigger::builder::time_interval(Duration::from_secs(1))
        .starting_at(Instant::now() + Duration::from_secs(3600))
        .update_interval(Duration::from_millis(100)) // IMPORTANT otherwise the new trigger is not applied until the next trigger of the current trigger
        .allow_manual_trigger()
        .build()
        .unwrap();

    let counters_init = counters.clone();
    let plugins = PluginSet::from(vec![PluginMetadata {
        name: PLUGIN_NAME.to_owned(),
        version: "0.0.1".to_owned(),
        init: Box::new(move |_| Ok(TestPlugin::init(PLUGIN_NAME, 7, state, counters_init, initial_trigger))),
        default_config: Box::new(|| Ok(None)),
    }]);

    let pipeline = pipeline::Builder::new();
    let agent = agent::Builder::from_pipeline(plugins, pipeline).build_and_start()?;
    let control = &agent
        .pipeline
        .control_handle()
        .with_plugin(PluginName(PLUGIN_NAME.into()));
    agent
        .pipeline
        .async_runtime()
        .block_on(test_scenario(control, counters))?;
    control.shutdown();
    agent.wait_for_shutdown(CONTROL_TIMEOUT)?;
    Ok(())
}

async fn test_scenario(control: &PluginControlHandle, counters: MeasurementCounters) -> anyhow::Result<()> {
    assert_eq!(
        counters.n_poll_called.load(Ordering::Relaxed),
        0,
        "the source should stay idle for now"
    );
    assert_eq!(counters.n_transform_in.load(Ordering::Relaxed), 0);
    assert_eq!(counters.n_transform_out.load(Ordering::Relaxed), 0);
    assert_eq!(counters.n_written.load(Ordering::Relaxed), 0);

    // Try to manually poll and check that it worked.
    poll_manually(control).await?;
    assert_eq!(
        counters.n_poll_called.load(Ordering::Relaxed),
        1,
        "manual trigger (before reconfig) should work"
    );

    // Reconfigure the source trigger.
    let hour = Duration::from_secs(3600);
    let trigger = TriggerSpec::builder(hour)
        .starting_at(Instant::now() + hour)
        .allow_manual_trigger() // IMPORTANT otherwise poll_manually does nothing
        .build()?;
    control
        .send_wait(
            request::source(SourceNamePattern::exact(PLUGIN_NAME, SOURCE_NAME)).set_trigger(trigger),
            CONTROL_TIMEOUT,
        )
        .await?;
    // Wait some time to be sure that the new trigger has been applied.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        counters.n_poll_called.load(Ordering::Relaxed),
        1,
        "the new trigger should not have expired yet"
    );

    // Try to manually poll after the reconfiguration.
    poll_manually(control).await?;
    assert_eq!(
        counters.n_poll_called.load(Ordering::Relaxed),
        2,
        "manual trigger (after reconfig) should work"
    );
    Ok(())
}

async fn poll_manually(control: &PluginControlHandle) -> anyhow::Result<()> {
    control
        .send_wait(
            request::source(SourceNamePattern::exact(PLUGIN_NAME, SOURCE_NAME)).trigger_now(),
            CONTROL_TIMEOUT,
        )
        .await?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    Ok(())
}
