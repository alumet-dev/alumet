use std::{cell::RefCell, ops::Deref, rc::Rc, time::Duration};

use fxhash::FxHashMap;
use tokio::sync::RwLockReadGuard;
use wrapped_output::{OutputDone, SetOutputOutputCheck, WrappedOutput};
use wrapped_source::{SetSourceCheck, SourceDone, WrappedManagedSource};
use wrapped_transform::{SetTransformOutputCheck, TransformDone, WrappedTransform};

use crate::{
    agent::builder::TestExpectations,
    measurement::MeasurementBuffer,
    metrics::registry::MetricRegistry,
    pipeline::{
        control::{
            message::matching::{SourceMatcher, TransformMatcher},
            AnonymousControlHandle, ControlMessage,
        },
        elements::{
            output::builder::{BlockingOutputBuilder, OutputBuilder},
            source::{
                self,
                builder::{ManagedSourceBuilder, SourceBuilder},
                control::TriggerMessage,
                trigger,
            },
            transform::{self, builder::TransformBuilder},
        },
        matching::{SourceNamePattern, TransformNamePattern},
        naming::{OutputName, PluginName, SourceName, TransformName},
        Output,
    },
};

mod pretty;
mod wrapped_output;
mod wrapped_source;
mod wrapped_transform;

/// Structure representing runtimes expectations.
///
/// This structure contains the various components needed to
/// test an agent on runtime. It means test sources, do they retrieve correct data ?
/// It also means for transform plugins, are their output values correct depending on their input.
/// While [`StartupExpectations`] mainly focus on the test about correct agent initialization and
/// its metrics, transforms... This structure is used to test correct computation or gathering of values
///
///
#[derive(Default)]
pub struct RuntimeExpectations {
    auto_shutdown: bool,
    sources: FxHashMap<SourceName, Vec<SourceCheck>>,
    transforms: FxHashMap<TransformName, Vec<TransformCheck>>,
    outputs: FxHashMap<OutputName, Vec<OutputCheck>>,
}

pub(super) const TESTER_SOURCE_NAME: &str = "_tester";
pub(super) const TESTER_OUTPUT_NAME: &str = "_keep_alive";
pub(super) const TESTER_PLUGIN_NAME: &str = "_test_runtime_expectations";

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

        async fn disable_all_transforms(control: &AnonymousControlHandle) {
            log::debug!("Disabling transforms...");
            control
                .send(ControlMessage::Transform(transform::control::ControlMessage {
                    matcher: TransformMatcher::Name(TransformNamePattern::wildcard()),
                    new_state: transform::control::TaskState::Disabled,
                }))
                .await
                .unwrap();
            // TODO remove this hack: wait for the control command to be processed
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        async fn enable_transform(control: &AnonymousControlHandle, name: TransformName) {
            log::debug!("Enabling transforms...");
            control
                .send(ControlMessage::Transform(transform::control::ControlMessage {
                    matcher: TransformMatcher::Name(TransformNamePattern::exact(name.plugin(), name.transform())),
                    new_state: transform::control::TaskState::Enabled,
                }))
                .await
                .unwrap();
            // TODO remove this hack: wait for the control command to be processed
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        let source_tests_before = source_tests.clone();
        let transform_tests_before = transform_tests.clone();
        let output_tests_before = output_tests.clone();
        builder = builder.before_operation_begin(move |pipeline| {
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
                    SourceBuilder::Managed(builder) => {
                        let wrapped = wrap_managed_source_builder(
                            name,
                            checks,
                            builder,
                            source_tests_before.clone(),
                        );
                        SourceBuilder::Managed(wrapped)
                    },
                    a @ SourceBuilder::Autonomous(_) => a,
                }
            });

            // Add a special source that we will manually trigger in order to trigger transforms and outputs.
            log::debug!("adding test-controlled source {TESTER_SOURCE_NAME}");
            pipeline
                .add_source_builder(
                    PluginName(TESTER_PLUGIN_NAME.to_owned()),
                    TESTER_SOURCE_NAME,
                    SourceBuilder::Autonomous(Box::new(|_ctx, cancel, tx| {
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

            let task = async move {
                // Before testing sourcse, disable all transforms, so that they don't
                // process data that could interfere with the transform checks.
                disable_all_transforms(&control).await;

                // Test sources
                for (name, controller) in source_tests.into_iter() {
                    let SourceTestController {
                        checks,
                        set_tx,
                        mut done_rx,
                    } = controller;

                    if !checks.is_empty() {
                        log::debug!("Checking {name}...");
                    }

                    for check in checks {
                        // first, tell the source which test to execute
                        set_tx.send(SetSourceCheck(check)).await.unwrap();

                        // tell Alumet to trigger the source now
                        // message to send to Alumet to trigger the source
                        let trigger_msg =
                            ControlMessage::Source(source::control::ControlMessage::TriggerManually(TriggerMessage {
                                matcher: SourceMatcher::Name(SourceNamePattern::from(name.clone())),
                            }));
                        control.send(trigger_msg).await.unwrap();

                        // wait for the test to finish
                        if done_rx.recv().await.is_none() {
                            // the sender has been dropped: either a bug,
                            // or the wrapped source panicked (because the test failed)
                            break;
                        }
                    }
                }

                // Test transforms
                for (name, controller) in transform_tests.into_iter() {
                    let TransformTestController {
                        checks,
                        set_tx,
                        mut done_rx,
                    } = controller;

                    if !checks.is_empty() {
                        log::debug!("Checking {name}...");
                    }

                    for check in checks {
                        // enable only the transform we want
                        enable_transform(&control, check.name).await;

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

                        disable_all_transforms(&control).await;
                    }
                }

                // Before testing outputs, disable all transforms, so that we can pass data from
                // the tester source to the outputs without any modification.
                disable_all_transforms(&control).await;

                // Test outputs
                for (name, controller) in output_tests.into_iter() {
                    let OutputTestController {
                        checks,
                        set_tx,
                        mut done_rx,
                    } = controller;

                    if !checks.is_empty() {
                        log::debug!("Checking {name}...");
                    }

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
                            break;
                        }
                    }
                }

                // Shutdown the pipeline (can be disabled)
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

    /// Toggles automatic shutdown.
    ///
    /// If `auto_shutdown` is true, `RuntimeExpectations` will shutdown the Alumet pipeline
    /// after all the test cases have been executed.
    pub fn auto_shutdown(mut self, auto_shutdown: bool) -> Self {
        self.auto_shutdown = auto_shutdown;
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
    pub fn source_result<Fi, Fo>(mut self, source: SourceName, make_input: Fi, check_output: Fo) -> Self
    where
        Fi: Fn() + Send + 'static,
        Fo: Fn(&MeasurementBuffer) + Send + 'static,
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
    pub fn transform_result<Fi, Fo>(mut self, transform: TransformName, make_input: Fi, check_output: Fo) -> Self
    where
        Fi: Fn(&mut TransformCheckInputContext) -> MeasurementBuffer + Send + 'static,
        Fo: Fn(&MeasurementBuffer) + Send + 'static,
    {
        let name = transform.clone();
        self.transforms.entry(name.clone()).or_default().push(TransformCheck {
            name,
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
    /// 2. The output is triggered, its [`apply`](crate::pipeline::Output::write) method is called.
    /// 3. `check_output` is called.
    /// Here, you can check that the output is correct by reading files, etc.
    pub fn output_result<Fi, Fo>(mut self, output: OutputName, make_input: Fi, check_output: Fo) -> Self
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
    check_output: Box<dyn Fn(&MeasurementBuffer) + Send>,
}

pub struct TransformCheck {
    name: TransformName,
    make_input: Box<dyn Fn(&mut TransformCheckInputContext) -> MeasurementBuffer + Send>,
    check_output: Box<dyn Fn(&MeasurementBuffer) + Send>,
}

pub struct OutputCheck {
    make_input: Box<dyn Fn(&mut OutputCheckInputContext) -> MeasurementBuffer + Send>,
    check_output: Box<dyn Fn() + Send>,
}

pub struct TransformCheckInputContext<'a> {
    metrics: RwLockReadGuard<'a, MetricRegistry>,
}

impl<'a> TransformCheckInputContext<'a> {
    pub fn metrics(&'a self) -> &'a MetricRegistry {
        self.metrics.deref()
    }
}

pub struct OutputCheckInputContext<'a> {
    metrics: RwLockReadGuard<'a, MetricRegistry>,
}

impl<'a> OutputCheckInputContext<'a> {
    pub fn metrics(&'a self) -> &'a MetricRegistry {
        self.metrics.deref()
    }
}
