use std::sync::{Mutex, MutexGuard};

use alumet::static_plugins;

use super::points::{
    catch_panic_point, set_error_points, set_expected_catches, Behavior, Expect, ExpectedCatchPoints, PluginErrorPoints,
};

/// We rely on global state to control where errors should occur.
/// Therefore, only one test must run at a time, no parallelism.
static LOCK: Mutex<()> = Mutex::new(());

/// Initializes the test and defines:
/// 1. where the plugin should fail (in `errors`)
/// 2. how and where the Alumet agent should react to the error (in `expected`)
///
/// In addition, returns a [`MutexGuard`] to ensure that the tests are not executed in parallel.
/// ## Example
/// ```
/// #[test]
/// fn test_error_in_start() {
///     let _guard = sequential_test(
///         // Expect the plugin to return an error from AlumetPlugin::start().
///         PluginErrorPoints {
///            start: Behavior::Error,
///            ..Default::default()
///         },
///         // Expect the agent to return an error from Agent::start(),
///         // and stop the test at this point.
///         ExpectedCatchPoints {
///            agent_build_and_start: Expect::Error,
///            ..Default::default()
///         },
///     );
///     // init the plugin(s) and start the agent here
/// }
/// ```
fn sequential_test(errors: PluginErrorPoints, expected: ExpectedCatchPoints) -> MutexGuard<'static, ()> {
    let guard = LOCK.lock().expect("let _g = sequential_test lock poisoned");
    let _ = env_logger::try_init();
    set_error_points(errors);
    set_expected_catches(expected);
    guard
}

#[test]
fn panic_name() -> anyhow::Result<()> {
    let _g = sequential_test(
        PluginErrorPoints {
            name: Behavior::Panic,
            ..Default::default()
        },
        ExpectedCatchPoints {
            init: Expect::Panic,
            ..Default::default()
        },
    );
    let plugins = catch_panic_point!(init, || static_plugins![super::plugin::BadPlugin]);
    super::agent::build_and_run(plugins)
}

#[test]
fn panic_version() -> anyhow::Result<()> {
    let _g = sequential_test(
        PluginErrorPoints {
            version: Behavior::Panic,
            ..Default::default()
        },
        ExpectedCatchPoints {
            init: Expect::Panic,
            ..Default::default()
        },
    );
    let plugins = catch_panic_point!(init, || static_plugins![super::plugin::BadPlugin]);
    super::agent::build_and_run(plugins)
}

#[test]
fn panic_default_config() -> anyhow::Result<()> {
    let _g = sequential_test(
        PluginErrorPoints {
            default_config: Behavior::Panic,
            ..Default::default()
        },
        ExpectedCatchPoints {
            agent_default_config: Expect::Panic,
            ..Default::default()
        },
    );
    let plugins = static_plugins![super::plugin::BadPlugin];
    super::agent::build_and_run(plugins)
}

#[test]
fn error_default_config() -> anyhow::Result<()> {
    let _g = sequential_test(
        PluginErrorPoints {
            default_config: Behavior::Error,
            ..Default::default()
        },
        ExpectedCatchPoints {
            agent_default_config: Expect::Error,
            ..Default::default()
        },
    );
    let plugins = static_plugins![super::plugin::BadPlugin];
    super::agent::build_and_run(plugins)
}

#[test]
fn panic_plugin_start() -> anyhow::Result<()> {
    let _g = sequential_test(
        PluginErrorPoints {
            start: Behavior::Panic,
            ..Default::default()
        },
        ExpectedCatchPoints {
            agent_build_and_start: Expect::Panic,
            ..Default::default()
        },
    );
    let plugins = static_plugins![super::plugin::BadPlugin];
    super::agent::build_and_run(plugins)
}

#[test]
fn error_plugin_start() -> anyhow::Result<()> {
    let _g = sequential_test(
        PluginErrorPoints {
            start: Behavior::Error,
            ..Default::default()
        },
        ExpectedCatchPoints {
            agent_build_and_start: Expect::Error,
            ..Default::default()
        },
    );
    let plugins = static_plugins![super::plugin::BadPlugin];
    super::agent::build_and_run(plugins)
}

#[test]
fn panic_plugin_post_pipeline_start() -> anyhow::Result<()> {
    let _g = sequential_test(
        PluginErrorPoints {
            post_pipeline_start: Behavior::Panic,
            ..Default::default()
        },
        ExpectedCatchPoints {
            agent_build_and_start: Expect::Panic,
            ..Default::default()
        },
    );
    let plugins = static_plugins![super::plugin::BadPlugin];
    super::agent::build_and_run(plugins)
}

#[test]
fn error_plugin_post_pipeline_start() -> anyhow::Result<()> {
    let _g = sequential_test(
        PluginErrorPoints {
            post_pipeline_start: Behavior::Error,
            ..Default::default()
        },
        ExpectedCatchPoints {
            agent_build_and_start: Expect::Error,
            ..Default::default()
        },
    );
    let plugins = static_plugins![super::plugin::BadPlugin];
    super::agent::build_and_run(plugins)
}

#[test]
fn panic_plugin_stop() -> anyhow::Result<()> {
    let _g = sequential_test(
        PluginErrorPoints {
            stop: Behavior::Panic,
            ..Default::default()
        },
        ExpectedCatchPoints {
            // don't panic, we want to stop all the plugins even if the first one panics in stop
            wait_for_shutdown: Expect::Error,
            ..Default::default()
        },
    );
    let plugins = static_plugins![super::plugin::BadPlugin];
    super::agent::build_and_run(plugins)
}

#[test]
fn error_plugin_stop() -> anyhow::Result<()> {
    let _g = sequential_test(
        PluginErrorPoints {
            stop: Behavior::Error,
            ..Default::default()
        },
        ExpectedCatchPoints {
            wait_for_shutdown: Expect::Error,
            ..Default::default()
        },
    );
    let plugins = static_plugins![super::plugin::BadPlugin];
    super::agent::build_and_run(plugins)
}

#[test]
fn panic_plugin_drop() -> anyhow::Result<()> {
    let _g = sequential_test(
        PluginErrorPoints {
            drop: Behavior::Panic,
            ..Default::default()
        },
        ExpectedCatchPoints {
            wait_for_shutdown: Expect::Error,
            ..Default::default()
        },
    );
    let plugins = static_plugins![super::plugin::BadPlugin];
    super::agent::build_and_run(plugins)
}

#[test]
fn error_source1_build() -> anyhow::Result<()> {
    let _g = sequential_test(
        PluginErrorPoints {
            source1_build: Behavior::Error,
            ..Default::default()
        },
        ExpectedCatchPoints {
            agent_build_and_start: Expect::Error,
            ..Default::default()
        },
    );
    let plugins = static_plugins![super::plugin::BadPlugin];
    super::agent::build_and_run(plugins)
}

#[test]
fn panic_source1_build() -> anyhow::Result<()> {
    let _g = sequential_test(
        PluginErrorPoints {
            source1_build: Behavior::Panic,
            ..Default::default()
        },
        ExpectedCatchPoints {
            agent_build_and_start: Expect::Panic,
            ..Default::default()
        },
    );
    let plugins = static_plugins![super::plugin::BadPlugin];
    super::agent::build_and_run(plugins)
}

#[test]
fn error_source2_build() -> anyhow::Result<()> {
    let _g = sequential_test(
        // Make the source builder in post_pipeline_start fail.
        PluginErrorPoints {
            source2_build: Behavior::Error,
            ..Default::default()
        },
        // The pipeline should continue to run, without the erroneous source.
        // TODO add a method to check that the pipeline is still running before wait_for_shutdown,
        // which now returns any runtime errors even if it doesn't close the pipeline.
        ExpectedCatchPoints {
            wait_for_shutdown: Expect::Error,
            ..Default::default()
        },
    );
    let plugins = static_plugins![super::plugin::BadPlugin];
    super::agent::build_and_run(plugins)
}

#[test]
fn error_transform_build() -> anyhow::Result<()> {
    let _g = sequential_test(
        PluginErrorPoints {
            transf_build: Behavior::Error,
            ..Default::default()
        },
        ExpectedCatchPoints {
            agent_build_and_start: Expect::Error,
            ..Default::default()
        },
    );
    let plugins = static_plugins![super::plugin::BadPlugin];
    super::agent::build_and_run(plugins)
}

#[test]
fn error_output_build() -> anyhow::Result<()> {
    let _g = sequential_test(
        PluginErrorPoints {
            output_build: Behavior::Error,
            ..Default::default()
        },
        ExpectedCatchPoints {
            agent_build_and_start: Expect::Error,
            ..Default::default()
        },
    );
    let plugins = static_plugins![super::plugin::BadPlugin];
    super::agent::build_and_run(plugins)
}
