use std::{cell::RefCell, ops::Deref, rc::Rc};

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
        control::{message::matching::SourceMatcher, ControlMessage},
        elements::{
            output::builder::{BlockingOutputBuilder, OutputBuilder},
            source::{
                self,
                builder::{ManagedSourceBuilder, SourceBuilder},
                control::TriggerMessage,
                trigger,
            },
            transform::builder::TransformBuilder,
        },
        matching::SourceNamePattern,
        naming::{OutputName, PluginName, SourceName, TransformName},
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

        let source_tests_before = source_tests.clone();
        let transform_tests_before = transform_tests.clone();
        let output_tests_before = output_tests.clone();
        builder = builder.before_operation_begin(move |pipeline| {
            // Wrap the sources
            pipeline.replace_sources(|name, builder| {
                log::debug!("preparing {name} for testing");
                match self.sources.remove(&name) {
                    Some(checks) => match builder {
                        SourceBuilder::Managed(b) => SourceBuilder::Managed(wrap_managed_source_builder(
                            name,
                            checks,
                            b,
                            source_tests_before.clone(),
                        )),
                        a @ SourceBuilder::Autonomous(_) => a,
                    },
                    None => builder,
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
                match self.transforms.remove(&name) {
                    Some(checks) => wrap_transform_builder(name, checks, builder, transform_tests_before.clone()),
                    None => builder,
                }
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
                // Test sources
                for (name, controller) in source_tests.into_iter() {
                    log::debug!("Checking {name}...");
                    let SourceTestController {
                        checks,
                        set_tx,
                        mut done_rx,
                    } = controller;

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
                    log::debug!("Checking {name}...");
                    let TransformTestController {
                        checks,
                        set_tx,
                        mut done_rx,
                    } = controller;

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
                }

                // Test outputs
                for (name, controller) in output_tests.into_iter() {
                    log::debug!("Checking {name}...");
                    let OutputTestController {
                        checks,
                        set_tx,
                        mut done_rx,
                    } = controller;

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
