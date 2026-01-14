use std::{
    cell::RefCell,
    ops::Deref,
    rc::Rc,
    sync::{Arc, Mutex},
    time::Duration,
};

use rustc_hash::FxHashMap;
use tokio::sync::RwLockReadGuard;
use wrapped_output::{OutputDone, SetOutputOutputCheck, WrappedOutput};
use wrapped_source::{SetSourceCheck, SourceDone, WrappedManagedSource};
use wrapped_transform::{SetTransformOutputCheck, TransformDone, WrappedTransform};

use crate::{
    agent::builder::TestExpectations,
    measurement::{MeasurementBuffer, MeasurementType},
    metrics::{
        Metric, RawMetricId,
        duplicate::{DuplicateCriteria, DuplicateReaction},
        error::MetricCreationError,
        online::MetricReader,
        registry::MetricRegistry,
    },
    pipeline::{
        Output,
        control::{
            AnonymousControlHandle,
            request::{self, any::AnyAnonymousControlRequest},
        },
        elements::{
            output::builder::{BlockingOutputBuilder, OutputBuilder},
            source::{
                builder::{ManagedSourceBuilder, SourceBuilder},
                trigger,
            },
            transform::builder::TransformBuilder,
        },
        matching::{OutputNamePattern, SourceNamePattern, TransformNamePattern},
        naming::{OutputName, PluginName, SourceName, TransformName},
    },
    units::PrefixedUnit,
};

mod pretty;
mod wrapped_output;
mod wrapped_source;
mod wrapped_transform;

/// Structure representing runtime expectations.
///
/// `RuntimeExpectations` allows to define a set of tests to run while the measurement pipeline runs.
/// You can declare tests for sources, transforms and outputs.
///
/// # Test isolation
///
/// When a test is run, other pipeline elements may be disabled to ensure that the input that you provide
/// is passed to the tested element. Therefore, each test should only assess the behavior of one specific
/// element.
///
/// # Example
/// ```no_run
/// use std::time::Duration;
///
/// use alumet::agent;
/// use alumet::pipeline::naming::SourceName;
/// use alumet::test::RuntimeExpectations;
/// use alumet::units::Unit;
///
/// const TIMEOUT: Duration = Duration::from_secs(2);
///
/// // define the checks that you want to apply
/// let runtime = RuntimeExpectations::new()
///     // test a source
///     .test_source(
///         SourceName::from_str("plugin_to_test", "source_to_test"),
///         || {
///             // Prepare the environment for the test
///             todo!();
///         },
///         |out| {
///             // The source has been triggered by the test module, check its output.
///             // As an example, we check that the source has produced only one point.
///             assert_eq!(out.measurements().len(), 1);
///             todo!();
///         },
///     );
///
/// // start an Alumet agent
/// let plugins = todo!();
/// let agent = agent::Builder::new(plugins)
///     .with_expectations(runtime) // load the checks
///     .build_and_start()
///     .unwrap();
///
/// // stop the agent
/// agent.pipeline.control_handle().shutdown();
/// // wait for the agent to stop
/// agent.wait_for_shutdown(TIMEOUT).unwrap();
/// ```
#[derive(Default)]
pub struct RuntimeExpectations {
    auto_shutdown: bool,
    metrics_to_create: Vec<Metric>,

    sources: FxHashMap<SourceName, Vec<SourceCheck>>,
    transforms: FxHashMap<TransformName, Vec<TransformCheck>>,
    outputs: FxHashMap<OutputName, Vec<OutputCheck>>,
}

// To conduct the tests, we need to insert some elements into the pipeline.
// Since we need to name these elements, we use the special names below.
pub(super) const TESTER_SOURCE_NAME: &str = "_tester";
pub(super) const TESTER_OUTPUT_NAME: &str = "_keep_alive";
pub(super) const TESTER_PLUGIN_NAME: &str = "_test_runtime_expectations";

const CONTROL_TIMEOUT: Duration = Duration::from_millis(100);

type TestControllerMap<N, C> = Rc<RefCell<FxHashMap<N, C>>>;

struct SourceTestController {
    checks: Vec<SourceCheck>,
    set_tx: tokio::sync::mpsc::Sender<SetSourceCheck>,
    done_rx: tokio::sync::mpsc::Receiver<SourceDone>,
}
struct TransformTestController {
    checks: Vec<TransformCheck>,
    set_tx: tokio::sync::mpsc::Sender<SetTransformOutputCheck>,
    done_rx: tokio::sync::mpsc::Receiver<TransformDone>,
}
struct OutputTestController {
    checks: Vec<OutputCheck>,
    set_tx: tokio::sync::mpsc::Sender<SetOutputOutputCheck>,
    done_rx: tokio::sync::mpsc::Receiver<OutputDone>,
}

impl TestExpectations for RuntimeExpectations {
    fn setup(mut self, mut builder: crate::agent::Builder) -> crate::agent::Builder {
        let source_tests: TestControllerMap<SourceName, SourceTestController> =
            Rc::new(RefCell::new(FxHashMap::default()));

        let transform_tests: TestControllerMap<TransformName, TransformTestController> =
            Rc::new(RefCell::new(FxHashMap::default()));

        let output_tests: TestControllerMap<OutputName, OutputTestController> =
            Rc::new(RefCell::new(FxHashMap::default()));

        let (tester_tx, mut tester_rx) = tokio::sync::mpsc::channel(2);

        fn wrap_managed_source_builder(
            name: SourceName,
            checks: Vec<SourceCheck>,
            builder: Box<dyn ManagedSourceBuilder>,
            controllers: TestControllerMap<SourceName, SourceTestController>,
            metrics_reader: Arc<Mutex<Option<MetricReader>>>,
        ) -> Box<dyn ManagedSourceBuilder> {
            Box::new(move |ctx| {
                let mut source = builder(ctx)?;

                source.trigger_spec = trigger::builder::manual() // don't trigger with a timer, only manually
                    .flush_rounds(1) // flush immediately
                    .update_rounds(1) // update asap
                    .build()?;
                log::trace!("trigger of {name} replaced by: {:?}", source.trigger_spec);

                // create the channels that we use to prevent multiple source tests from running at the same time
                let (set_tx, set_rx) = tokio::sync::mpsc::channel(1);
                let (done_tx, done_rx) = tokio::sync::mpsc::channel(1);
                controllers.borrow_mut().insert(
                    name,
                    SourceTestController {
                        checks,
                        set_tx,
                        done_rx,
                    },
                );

                // wrap the source
                source.source = Box::new(WrappedManagedSource {
                    source: source.source,
                    in_rx: set_rx,
                    out_tx: done_tx,
                    metrics_r: metrics_reader.clone(),
                });
                Ok(source)
            })
        }

        fn wrap_transform_builder(
            name: TransformName,
            checks: Vec<TransformCheck>,
            builder: Box<dyn TransformBuilder>,
            controllers: TestControllerMap<TransformName, TransformTestController>,
        ) -> Box<dyn TransformBuilder> {
            Box::new(move |ctx| {
                let transform = builder(ctx)?;

                // create the channels that we use to prevent multiple source tests from running at the same time
                let (set_tx, set_rx) = tokio::sync::mpsc::channel(1);
                let (done_tx, done_rx) = tokio::sync::mpsc::channel(1);
                controllers.borrow_mut().insert(
                    name,
                    TransformTestController {
                        checks,
                        set_tx,
                        done_rx,
                    },
                );

                // wrap the transform
                let transform = Box::new(WrappedTransform {
                    transform,
                    set_rx,
                    done_tx,
                });
                Ok(transform)
            })
        }

        fn wrap_blocking_output_builder(
            name: OutputName,
            checks: Vec<OutputCheck>,
            builder: Box<dyn BlockingOutputBuilder>,
            controllers: TestControllerMap<OutputName, OutputTestController>,
        ) -> Box<dyn BlockingOutputBuilder> {
            Box::new(move |ctx| {
                let output = builder(ctx)?;

                // create the channels that we use to prevent multiple source tests from running at the same time
                let (set_tx, set_rx) = tokio::sync::mpsc::channel(1);
                let (done_tx, done_rx) = tokio::sync::mpsc::channel(1);
                controllers.borrow_mut().insert(
                    name,
                    OutputTestController {
                        checks,
                        set_tx,
                        done_rx,
                    },
                );

                // wrap the output
                let output = Box::new(WrappedOutput {
                    output,
                    set_rx,
                    done_tx,
                });
                Ok(output)
            })
        }

        let source_tests_before = source_tests.clone();
        let transform_tests_before = transform_tests.clone();
        let output_tests_before = output_tests.clone();
        let metrics_to_create = self.metrics_to_create;

        let metrics_reader_for_sources: Arc<Mutex<Option<MetricReader>>> = Arc::new(Mutex::new(None));

        builder = builder.before_operation_begin(move |pipeline| {
            // Create the test metrics
            let res = pipeline.metrics.register_many(metrics_to_create, DuplicateCriteria::Strict, DuplicateReaction::Error);
            let res: Result<Vec<RawMetricId>, MetricCreationError> = res.into_iter().collect();
            res.expect("failed to create the test metrics, check that there is no duplicate (strict)");

            // Wrap the sources
            pipeline.replace_sources(|name, builder| {
                log::debug!("preparing {name} for testing");
                let checks = self.sources.remove(&name).unwrap_or_default();
                // Even if the source has no associated check, we must replace it to prevent it
                // from running in an uncontrolled way. All the sources must be triggered only
                // when we determine it's okay to do so, otherwise it will interfere with
                // transform and output testing.
                // TODO this may be revisited when/if MeasurementBuffer is augmented with the
                // origin of the measurements.
                match builder {
                    SourceBuilder::Managed(builder, s) => {
                        let wrapped = wrap_managed_source_builder(
                            name,
                            checks,
                            builder,
                            source_tests_before.clone(),
                            metrics_reader_for_sources.clone(),
                        );
                        SourceBuilder::Managed(wrapped, s)
                    },
                    a @ SourceBuilder::Autonomous(_) => a,
                }
            });

            // TODO Assert that all the sources that we want to test already exist?
            // assert!(self.sources.is_empty(), "these sources should exist on startup: {}", self.sources.iter().map(|s| s.0.to_string()).collect::<Vec<_>>().join(", "));

            // Add a special source that we will manually trigger in order to trigger transforms and outputs.
            log::debug!("adding test-controlled source {TESTER_SOURCE_NAME}");
            pipeline
                .add_source_builder(
                    PluginName(TESTER_PLUGIN_NAME.to_owned()),
                    TESTER_SOURCE_NAME,
                    SourceBuilder::Autonomous(Box::new(move |ctx, cancel, tx| {
                        // populate the MetricReader that we need in the wrapped sources
                        *metrics_reader_for_sources.lock().unwrap() = Some(ctx.metrics_reader());

                        // create the special source
                        Ok(Box::pin(async move {
                            loop {
                                tokio::select! {
                                    biased;
                                    _ = cancel.cancelled() => {
                                        log::debug!("{TESTER_SOURCE_NAME} has been cancelled");
                                        break;
                                    },
                                    m = tester_rx.recv() => {
                                        if let Some(measurements) = m {
                                            log::debug!("{TESTER_SOURCE_NAME} sends new measurements: {measurements:?}");
                                            tx.send(measurements).await.unwrap();
                                        } else {
                                            log::debug!("{TESTER_SOURCE_NAME} channel sender has been closed");
                                            break;
                                        }
                                    }
                                }
                            }
                            Ok(())
                        }))
                    })),
                )
                .unwrap();

            // Wrap the transforms
            pipeline.replace_transforms(|name, builder| {
                log::debug!("preparing {name} for testing");
                // Similar to sources, every transform must be wrapped to prevent any interference with output checks.
                let checks = self.transforms.remove(&name).unwrap_or_default();
                wrap_transform_builder(name, checks, builder, transform_tests_before.clone())
            });

            // Wrap the outputs
            pipeline.replace_outputs(|name, builder| {
                log::debug!("preparing {name} for testing");
                match self.outputs.remove(&name) {
                    Some(checks) => match builder {
                        OutputBuilder::Blocking(b) => OutputBuilder::Blocking(wrap_blocking_output_builder(
                            name,
                            checks,
                            b,
                            output_tests_before.clone(),
                        )),
                        a @ OutputBuilder::Async(_) => a,
                    },
                    None => builder,
                }
            });

            // Add a special output to keep the pipeline alive when all the outputs added by the plugin fail.
            // This is because we want to report only the output error, not errors caused by the lack of outputs (sources and transforms will panic).
            log::debug!("adding test-controlled output {TESTER_OUTPUT_NAME}");
            pipeline.add_output_builder(PluginName(TESTER_PLUGIN_NAME.to_owned()), TESTER_OUTPUT_NAME, OutputBuilder::Blocking(Box::new(|_ctx| {
                use crate::pipeline::elements::output::OutputContext;
                use crate::pipeline::elements::error::WriteError;
                struct DummyOutput;
                impl Output for DummyOutput {
                    fn write(&mut self, _measurements: &MeasurementBuffer, _ctx: &OutputContext) -> Result<(), WriteError> {
                        // do nothing
                        Ok(())
                    }
                }
                Ok(Box::new(DummyOutput))
            }))).unwrap();
        });

        // IMPORTANT: disallow simplified pipeline
        *builder.pipeline().allow_simplified_pipeline() = false;

        // Setup a background task that will trigger the elements one by one for testing purposes.
        builder.after_operation_begin(move |pipeline| {
            let control = pipeline.control_handle();
            let mr = pipeline.metrics_reader().clone();

            let source_tests = source_tests.take();
            let transform_tests = transform_tests.take();
            let output_tests = output_tests.take();

            log::debug!(
                "source_tests: {}",
                source_tests
                    .keys()
                    .map(|n| n.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            log::debug!(
                "transform_tests: {}",
                transform_tests
                    .keys()
                    .map(|n| n.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            log::debug!(
                "output_tests: {}",
                output_tests
                    .keys()
                    .map(|n| n.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );

            async fn send_requests(control: &AnonymousControlHandle, requests: Vec<AnyAnonymousControlRequest>) {
                for r in requests {
                    control
                        .send_wait(r, CONTROL_TIMEOUT)
                        .await
                        .expect("control request failed");
                }
            }

            let task = async move {
                // Disable everything, except the special output (in order to consume the measurements).
                log::debug!("Disabling every pipeline element.");
                let requests: Vec<AnyAnonymousControlRequest> = vec![
                    request::source(SourceNamePattern::wildcard()).disable().into(),
                    request::transform(TransformNamePattern::wildcard()).disable().into(),
                    request::output(OutputNamePattern::wildcard()).disable().into(),
                    request::output(OutputNamePattern::exact(TESTER_PLUGIN_NAME, TESTER_OUTPUT_NAME))
                        .enable()
                        .into(),
                ];
                send_requests(&control, requests).await;

                // Test sources in isolation
                log::debug!("Testing sources…");
                for (name, controller) in source_tests.into_iter() {
                    let SourceTestController {
                        checks,
                        set_tx,
                        mut done_rx,
                    } = controller;

                    if checks.is_empty() {
                        continue;
                    }

                    log::debug!("Checking {name}...");

                    // enable the source
                    control
                        .send_wait(request::source(name.clone()).enable(), CONTROL_TIMEOUT)
                        .await
                        .unwrap();

                    for check in checks {
                        // first, tell the source which test to execute
                        set_tx.send(SetSourceCheck(check)).await.unwrap();

                        // tell Alumet to trigger the source now
                        // message to send to Alumet to trigger the source
                        let trigger = request::source(name.clone()).trigger_now();
                        control.dispatch(trigger, CONTROL_TIMEOUT).await.unwrap();

                        // wait for the test to finish
                        if done_rx.recv().await.is_none() {
                            // the sender has been dropped: either a bug,
                            // or the wrapped source panicked (because the test failed)
                            break;
                        }
                    }

                    // disable the source
                    control
                        .send_wait(request::source(name).disable(), CONTROL_TIMEOUT)
                        .await
                        .unwrap();
                }

                // From now on, we will use the special "tester" source to send arbitrary data to transform steps and outputs.

                // Test transforms in isolation
                log::debug!("Testing transforms…");
                for (name, controller) in transform_tests.into_iter() {
                    let TransformTestController {
                        checks,
                        set_tx,
                        mut done_rx,
                    } = controller;

                    if checks.is_empty() {
                        continue;
                    }

                    log::debug!("Checking {name}...");

                    // enable the transform
                    control
                        .send_wait(request::transform(name.clone()).enable(), CONTROL_TIMEOUT)
                        .await
                        .unwrap();

                    for check in checks {
                        // tell the transform which check to execute
                        set_tx.send(SetTransformOutputCheck(check.check_output)).await.unwrap();

                        // build the test input with user-provided code
                        let lock = mr.read().await;
                        let mut ctx = TransformCheckInputContext { metrics: lock };
                        let test_data = (check.make_input)(&mut ctx);

                        // trigger the "tester" source with the test input
                        tester_tx.send(test_data).await.unwrap();

                        // wait for the test to finish
                        if done_rx.recv().await.is_none() {
                            // the sender has been dropped: either a bug,
                            // or the wrapped transform panicked (because the test failed)
                            break;
                        }
                    }

                    // disable the transform
                    control
                        .send_wait(request::transform(name.clone()).disable(), CONTROL_TIMEOUT)
                        .await
                        .unwrap();
                }

                // Test outputs
                log::debug!("Testing outputs…");
                for (name, controller) in output_tests.into_iter() {
                    let OutputTestController {
                        checks,
                        set_tx,
                        mut done_rx,
                    } = controller;

                    if checks.is_empty() {
                        continue;
                    }

                    log::debug!("Checking {name}...");

                    // Enable the output and discard any pending data, otherwise
                    // the output will see the measurements sent by the tested sources and
                    // by the special tester source to the tested transforms.
                    control
                        .send_wait(request::output(name.clone()).enable_discard(), CONTROL_TIMEOUT)
                        .await
                        .unwrap();

                    for check in checks {
                        // tell the output which check to execute
                        set_tx.send(SetOutputOutputCheck(check.check_output)).await.unwrap();

                        // build the test input with user-provided code
                        let lock = mr.read().await;
                        let mut ctx = OutputCheckInputContext { metrics: lock };
                        let test_data = (check.make_input)(&mut ctx);

                        // trigger the "tester" source with the test input
                        tester_tx.send(test_data).await.unwrap();

                        // wait for the test to finish
                        if done_rx.recv().await.is_none() {
                            // the sender has been dropped: either a bug,
                            // or the wrapped output panicked (because the test failed)
                            log::warn!("done_tx has been dropped");
                            break;
                        }
                    }

                    // disable the output
                    control
                        .send_wait(request::output(name.clone()).disable(), CONTROL_TIMEOUT)
                        .await
                        .unwrap();
                }

                // Tests are done, shutdown the pipeline if requested to do so.
                if self.auto_shutdown {
                    log::debug!("Requesting shutdown...");
                    control.shutdown();
                } else {
                    log::debug!("Not requesting shutdown. Do you shutdown the pipeline yourself?");
                }
            };
            pipeline.async_runtime().spawn(task);
        })
    }
}

impl RuntimeExpectations {
    pub fn new() -> Self {
        Self {
            auto_shutdown: true,
            ..Default::default()
        }
    }

    /// Toggles automatic shutdown (it is enabled by default).
    ///
    /// If `auto_shutdown` is true, `RuntimeExpectations` will shutdown the Alumet pipeline
    /// after all the test cases have been executed.
    pub fn auto_shutdown(mut self, auto_shutdown: bool) -> Self {
        self.auto_shutdown = auto_shutdown;
        self
    }

    /// Creates a new metric (after the plugins have been initialized).
    pub fn create_metric<T: MeasurementType>(mut self, name: impl Into<String>, unit: impl Into<PrefixedUnit>) -> Self {
        let m = Metric {
            name: name.into(),
            description: "".into(),
            value_type: T::wrapped_type(),
            unit: unit.into(),
        };
        self.metrics_to_create.push(m);
        self
    }

    /// Registers a new test case for a source.
    ///
    /// # Execution of a source test
    /// 1. `make_input` is called to prepare the environment of the source.
    /// Here, you can write to files, modify global variables, etc.
    /// 2. The source is triggered, its [`poll`](crate::pipeline::Source::poll) method is called.
    /// 3. `check_output` is called with the measurements produced by the source.
    /// Here, you can check that the result is correct using usual assertions such as [`assert_eq`].
    pub fn test_source<Fi, Fo>(mut self, source: SourceName, make_input: Fi, check_output: Fo) -> Self
    where
        Fi: Fn() + Send + 'static,
        Fo: Fn(&mut SourceCheckOutputContext) + Send + 'static,
    {
        let name = source.clone();
        self.sources.entry(name).or_default().push(SourceCheck {
            make_input: Box::new(make_input),
            check_output: Box::new(check_output),
        });
        self
    }

    /// Registers a new test case for a transform.
    ///
    /// # Execution of a transform test
    /// 1. `make_input` is called to prepare the input of the transform.
    /// It adds measurements to a buffer, that will be given to the transform.
    /// 2. The transform is triggered, its [`apply`](crate::pipeline::Transform::apply) method is called.
    /// 3. `check_output` is called with the buffer modified by the transform.
    /// Here, you can check that the result is correct using usual assertions such as `assert_eq!`.
    pub fn test_transform<Fi, Fo>(mut self, transform: TransformName, make_input: Fi, check_output: Fo) -> Self
    where
        Fi: Fn(&mut TransformCheckInputContext) -> MeasurementBuffer + Send + 'static,
        Fo: Fn(&mut TransformCheckOutputContext) + Send + 'static,
    {
        let name = transform.clone();
        self.transforms.entry(name).or_default().push(TransformCheck {
            make_input: Box::new(make_input),
            check_output: Box::new(check_output),
        });
        self
    }

    /// Registers a new test case for an output.
    ///
    /// # Execution of an output test
    /// 1. `make_input` is called to prepare the input of the output.
    /// It adds measurements to a buffer, that will be given to the output.
    /// 2. The output is triggered, its [`write`](crate::pipeline::Output::write) method is called.
    /// 3. `check_output` is called.
    /// Here, you can check that the output is correct by reading files, etc.
    pub fn test_output<Fi, Fo>(mut self, output: OutputName, make_input: Fi, check_output: Fo) -> Self
    where
        Fi: Fn(&mut OutputCheckInputContext) -> MeasurementBuffer + Send + 'static,
        Fo: Fn() + Send + 'static,
    {
        let name = output.clone();
        self.outputs.entry(name).or_default().push(OutputCheck {
            make_input: Box::new(make_input),
            check_output: Box::new(check_output),
        });
        self
    }
}

pub struct SourceCheck {
    make_input: Box<dyn Fn() + Send>,
    check_output: Box<dyn Fn(&mut SourceCheckOutputContext) + Send>,
}

pub struct TransformCheck {
    make_input: Box<dyn Fn(&mut TransformCheckInputContext) -> MeasurementBuffer + Send>,
    check_output: Box<dyn Fn(&mut TransformCheckOutputContext) + Send>,
}

pub struct OutputCheck {
    make_input: Box<dyn Fn(&mut OutputCheckInputContext) -> MeasurementBuffer + Send>,
    check_output: Box<dyn Fn() + Send>,
}

// test_source ctx

pub struct SourceCheckOutputContext<'a> {
    measurements: &'a MeasurementBuffer,
    metrics: &'a MetricRegistry,
}

impl<'a> SourceCheckOutputContext<'a> {
    pub fn measurements(&self) -> &MeasurementBuffer {
        self.measurements
    }

    pub fn metrics(&'a self) -> &'a MetricRegistry {
        self.metrics
    }
}

// test_transform ctx

pub struct TransformCheckInputContext<'a> {
    metrics: RwLockReadGuard<'a, MetricRegistry>,
}

impl<'a> TransformCheckInputContext<'a> {
    pub fn metrics(&'a self) -> &'a MetricRegistry {
        self.metrics.deref()
    }
}

pub struct TransformCheckOutputContext<'a> {
    measurements: &'a MeasurementBuffer,
    metrics: &'a MetricRegistry,
}

impl<'a> TransformCheckOutputContext<'a> {
    pub fn measurements(&self) -> &MeasurementBuffer {
        self.measurements
    }

    pub fn metrics(&'a self) -> &'a MetricRegistry {
        self.metrics
    }
}

// test_output ctx

pub struct OutputCheckInputContext<'a> {
    metrics: RwLockReadGuard<'a, MetricRegistry>,
}

impl<'a> OutputCheckInputContext<'a> {
    pub fn metrics(&'a self) -> &'a MetricRegistry {
        self.metrics.deref()
    }
}
