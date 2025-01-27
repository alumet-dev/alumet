use std::{cell::RefCell, ops::Deref, rc::Rc, sync::mpsc, time::Duration};

use fxhash::FxHashMap;
use tokio::sync::RwLockReadGuard;
use wrapped_output::{OutputDone, SetOutputOutputCheck, WrappedOutput};
use wrapped_source::{SetSourceCheck, SourceDone, WrappedManagedSource};
use wrapped_transform::{SetTransformOutputCheck, TransformDone, WrappedTransform};

use crate::{
    agent::builder::TestBuilderVisitor,
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
            },
            transform::builder::TransformBuilder,
        },
        matching::SourceNamePattern,
        naming::{OutputName, PluginName, SourceName, TransformName},
        trigger::TriggerConstraints,
    },
};

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
    sources: FxHashMap<SourceName, Vec<SourceCheck>>,
    transforms: FxHashMap<TransformName, Vec<TransformCheck>>,
    outputs: FxHashMap<OutputName, Vec<OutputCheck>>,
}

type TestControllerMap<N, C> = Rc<RefCell<FxHashMap<N, C>>>;

struct SourceTestController {
    checks: Vec<SourceCheck>,
    set_tx: mpsc::Sender<SetSourceCheck>,
    done_rx: mpsc::Receiver<SourceDone>,
}
struct TransformTestController {
    checks: Vec<TransformCheck>,
    set_tx: mpsc::Sender<SetTransformOutputCheck>,
    done_rx: mpsc::Receiver<TransformDone>,
}
struct OutputTestController {
    checks: Vec<OutputCheck>,
    set_tx: mpsc::Sender<SetOutputOutputCheck>,
    done_rx: mpsc::Receiver<OutputDone>,
}

impl TestBuilderVisitor for RuntimeExpectations {
    fn visit(mut self, mut builder: crate::agent::Builder) -> crate::agent::Builder {
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

                // force `allow_manual_trigger` to true
                let constraints = TriggerConstraints {
                    max_update_interval: Duration::MAX,
                    allow_manual_trigger: true,
                };
                source.trigger_spec.constrain(&constraints);

                // create the channels that we use to prevent multiple source tests from running at the same time
                let (set_tx, set_rx) = mpsc::channel();
                let (done_tx, done_rx) = mpsc::channel();
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
                let (set_tx, set_rx) = mpsc::channel();
                let (done_tx, done_rx) = mpsc::channel();
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
                let (set_tx, set_rx) = mpsc::channel();
                let (done_tx, done_rx) = mpsc::channel();
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

        // Wrap the sources
        builder
            .pipeline()
            .inspect()
            .replace_sources(|name, builder| match self.sources.remove(&name) {
                Some(checks) => match builder {
                    SourceBuilder::Managed(b) => {
                        SourceBuilder::Managed(wrap_managed_source_builder(name, checks, b, source_tests.clone()))
                    }
                    a @ SourceBuilder::Autonomous(_) => a,
                },
                None => builder,
            });

        // Add a special source that we will manually trigger in order to trigger transforms and outputs.
        let tester_source_name = SourceName::new(String::from("test_runtime_expectations"), String::from("tester"));
        builder
            .pipeline()
            .add_source_builder(
                PluginName(tester_source_name.plugin().to_owned()),
                tester_source_name.source(),
                SourceBuilder::Autonomous(Box::new(|ctx, cancel, tx| {
                    Ok(Box::pin(async move {
                        let measurements: MeasurementBuffer = tester_rx.recv().await.unwrap();
                        tx.send(measurements).await.unwrap();
                        Ok(())
                    }))
                })),
            )
            .unwrap();

        // Wrap the transforms
        builder
            .pipeline()
            .inspect()
            .replace_transforms(|name, builder| match self.transforms.remove(&name) {
                Some(checks) => wrap_transform_builder(name, checks, builder, transform_tests.clone()),
                None => builder,
            });

        // Wrap the outputs
        builder
            .pipeline()
            .inspect()
            .replace_outputs(|name, builder| match self.outputs.remove(&name) {
                Some(checks) => match builder {
                    OutputBuilder::Blocking(b) => {
                        OutputBuilder::Blocking(wrap_blocking_output_builder(name, checks, b, output_tests.clone()))
                    }
                    a @ OutputBuilder::Async(_) => a,
                },
                None => builder,
            });

        // Setup a background task that will trigger the elements one by one for testing purposes.
        builder.after_operation_begin(move |pipeline| {
            let control = pipeline.control_handle();
            let mr = pipeline.metrics_reader().clone();

            let source_tests = source_tests.take();
            let transform_tests = transform_tests.take();
            let output_tests = output_tests.take();

            let task = async move {
                // Test sources
                for (name, controller) in source_tests.into_iter() {
                    let SourceTestController {
                        checks,
                        set_tx,
                        done_rx,
                    } = controller;

                    for check in checks {
                        // first, tell the source which test to execute
                        set_tx.send(SetSourceCheck(check)).unwrap();

                        // tell Alumet to trigger the source now
                        // message to send to Alumet to trigger the source
                        let trigger_msg =
                            ControlMessage::Source(source::control::ControlMessage::TriggerManually(TriggerMessage {
                                matcher: SourceMatcher::Name(SourceNamePattern::from(name.clone())),
                            }));
                        control.send(trigger_msg).await.unwrap();

                        // wait for the test to finish
                        done_rx.recv().unwrap();
                    }
                }

                // Test transforms
                for (name, controller) in transform_tests.into_iter() {
                    let TransformTestController {
                        checks,
                        set_tx,
                        done_rx,
                    } = controller;

                    for check in checks {
                        // tell the transform which check to execute
                        set_tx.send(SetTransformOutputCheck(check.check_output)).unwrap();

                        // build the test input with user-provided code
                        let lock = mr.read().await;
                        let mut ctx = TransformCheckInputContext { metrics: lock };
                        let test_data = (check.make_input)(&mut ctx);

                        // trigger the "tester" source with the test input
                        tester_tx.send(test_data).await.unwrap();

                        // wait for the test to finish
                        done_rx.recv().unwrap();
                    }
                }

                // Test outputs
                for (name, controller) in output_tests.into_iter() {
                    let OutputTestController {
                        checks,
                        set_tx,
                        done_rx,
                    } = controller;

                    for check in checks {
                        // tell the transform which check to execute
                        set_tx.send(SetOutputOutputCheck(check.check_output)).unwrap();

                        // build the test input with user-provided code
                        let lock = mr.read().await;
                        let mut ctx = OutputCheckInputContext { metrics: lock };
                        let test_data = (check.make_input)(&mut ctx);

                        // trigger the "tester" source with the test input
                        tester_tx.send(test_data).await.unwrap();

                        // wait for the test to finish
                        done_rx.recv().unwrap();
                    }
                }
            };
            pipeline.async_runtime().spawn(task);
        });
        todo!()
    }
}

impl RuntimeExpectations {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn source_result<Fi, Fo>(mut self, source: SourceName, make_input: Fi, check_output: Fo) -> Self
    where
        Fi: Fn() + Send + 'static,
        Fo: Fn(&MeasurementBuffer) + Send + 'static,
    {
        let name = source.clone();
        self.sources.entry(name).or_default().push(SourceCheck {
            source,
            make_input: Box::new(make_input),
            check_output: Box::new(check_output),
        });
        self
    }

    pub fn transform_result<Fi, Fo>(mut self, transform: TransformName, make_input: Fi, check_output: Fo) -> Self
    where
        Fi: Fn(&mut TransformCheckInputContext) -> MeasurementBuffer + Send + 'static,
        Fo: Fn(&MeasurementBuffer) + Send + 'static,
    {
        let name = transform.clone();
        self.transforms.entry(name).or_default().push(TransformCheck {
            transform,
            make_input: Box::new(make_input),
            check_output: Box::new(check_output),
        });
        self
    }

    pub fn output_result<Fi, Fo>(mut self, output: OutputName, make_input: Fi, check_output: Fo) -> Self
    where
        Fi: Fn(&mut OutputCheckInputContext) -> MeasurementBuffer + Send + 'static,
        Fo: Fn() + Send + 'static,
    {
        let name = output.clone();
        self.outputs.entry(name).or_default().push(OutputCheck {
            output,
            make_input: Box::new(make_input),
            check_output: Box::new(check_output),
        });
        self
    }
}

pub struct SourceCheck {
    source: SourceName,
    make_input: Box<dyn Fn() + Send>,
    check_output: Box<dyn Fn(&MeasurementBuffer) + Send>,
}

pub struct TransformCheck {
    transform: TransformName,
    make_input: Box<dyn Fn(&mut TransformCheckInputContext) -> MeasurementBuffer + Send>,
    check_output: Box<dyn Fn(&MeasurementBuffer) + Send>,
}

pub struct OutputCheck {
    output: OutputName,
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
