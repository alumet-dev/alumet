//! Implementation of the measurement pipeline.

use std::collections::HashMap;
use std::ops::BitOrAssign;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context};
use left_right::{Absorb, ReadHandle, WriteHandle};

use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::{broadcast, mpsc};
use tokio::task::{JoinError, JoinHandle, JoinSet};
use tokio::time::timeout;
use tokio::{runtime::Runtime, sync::watch};
use tokio_util::sync::CancellationToken;

use crate::measurement::Timestamp;
use crate::metrics::{Metric, RawMetricId};
use crate::pipeline::scoped;
use crate::pipeline::trigger::TriggerReason;
use crate::{
    measurement::MeasurementBuffer,
    metrics::MetricRegistry,
    pipeline::{Output, Source},
};

use super::builder::{ConfiguredTransform, ElementType};
use super::trigger::{Trigger, TriggerSpec};
use super::{builder, control};
use super::{OutputContext, PollError, TransformError, WriteError};

/// A measurement pipeline that has not been started yet.
pub struct IdlePipeline {
    // Elements of the pipeline
    pub(super) sources: Vec<builder::ConfiguredSource>,
    pub(super) transforms: Vec<builder::ConfiguredTransform>,
    pub(super) outputs: Vec<builder::ConfiguredOutput>,
    pub(super) autonomous_sources: Vec<builder::ConfiguredAutonomousSource>,

    // Cancellation token to implement the graceful shutdown of autonomous sources.
    pub(super) autonomous_shutdown_token: CancellationToken,

    // tokio Runtimes that execute the tasks
    pub(super) rt_normal: Runtime,
    pub(super) rt_priority: Option<Runtime>,

    // registries
    pub(super) metrics: MetricRegistry,

    /// Channel: source -> transforms
    pub(super) from_sources: (mpsc::Sender<MeasurementBuffer>, mpsc::Receiver<MeasurementBuffer>),

    /// Broadcast queue to outputs
    pub(super) to_outputs: broadcast::Sender<OutputMsg>,
}

/// A command sent to one or multiple elements of the pipeline.
struct ElementCommand<T> {
    destination: MessageDestination,
    command: T,
}

/// Specifies the destination of the [`ElementCommand`].
#[derive(Clone)]
enum MessageDestination {
    /// Send the command to all the elements of this type
    /// (e.g. all sources in case of an [`ElementCommand<SourceCmd>`]).
    All,
    /// Send the command to all the elements of this type registered by a specific plugin.
    Plugin(String),
}

/// A measurement pipeline that is currently running.
pub struct RunningPipeline {
    // Keep the tokio runtimes alive
    _rt_normal: Runtime,
    _rt_priority: Option<Runtime>,

    /// Handle to the task that handles the shutdown of the pipeline.
    ///
    /// When this task finishes, the pipeline has shut down.
    shutdown_task_handle: Option<JoinHandle<()>>,

    /// Controls the pipeline.
    control_handle: control::ControlHandle,
}

impl IdlePipeline {
    pub fn metric_count(&self) -> usize {
        self.metrics.len()
    }

    pub fn metric_iter(&self) -> crate::metrics::MetricIter<'_> {
        self.metrics.iter()
    }

    /// Starts the measurement pipeline.
    pub fn start(self) -> RunningPipeline {
        // Use a JoinSet to keep track of the spawned tasks.
        let mut source_set = JoinSet::new();
        let mut transform_set = JoinSet::new();
        let mut output_set = JoinSet::new();

        // Store the command senders in order to keep the receivers alive,
        // and to be able to send commands after the launch.
        let mut source_command_senders_by_plugin: HashMap<_, Vec<_>> = HashMap::new();
        let mut output_command_senders_by_plugin: HashMap<_, Vec<_>> = HashMap::new();
        let mut transforms_mask_by_plugin: HashMap<_, u64> = HashMap::new();

        // Start the tasks, starting at the end of the pipeline (to avoid filling the buffers too quickly).
        let (in_tx, in_rx) = self.from_sources;

        // 1. Outputs
        for out in self.outputs {
            let msg_rx = self.to_outputs.subscribe();
            let (command_tx, command_rx) = watch::channel(OutputCmd::Run);
            let ctx = OutputContext {
                // Each output task owns its OutputContext, which contains a copy of the MetricRegistry.
                // This allows fast, uncontended access to the registry, and avoids a global state (no Arc<Mutex<...>>).
                // The cost is a duplication of the registry (increased memory use) in the case where multiple outputs exist.
                metrics: self.metrics.clone(),
            };

            // Store command_tx so that we can accept commands later (commands can target the outputs of a specific plugin).
            output_command_senders_by_plugin
                .entry(out.plugin_name)
                .or_default()
                .push(command_tx);

            // Spawn the task in the JoinSet.
            let task = run_output_from_broadcast(out.name, out.output, msg_rx, command_rx, ctx);
            output_set.spawn_on(task, self.rt_normal.handle());
        }

        // 2. Transforms (all in the same task because they are applied one after another)
        let active_transforms = Arc::new(AtomicU64::new(u64::MAX)); // all active by default
        for (i, t) in self.transforms.iter().enumerate() {
            let mask: u64 = 1 << i;
            transforms_mask_by_plugin
                .entry(t.plugin_name.clone())
                .or_default()
                .bitor_assign(mask);
        }
        let transforms_task = run_transforms(self.transforms, in_rx, self.to_outputs, active_transforms.clone());
        transform_set.spawn_on(transforms_task, self.rt_normal.handle());

        // 3. Managed sources
        for src in self.sources {
            let data_tx = in_tx.clone();
            let runtime = match src.trigger_provider.realtime_priority {
                true => self.rt_priority.as_ref().unwrap_or(&self.rt_normal),
                false => &self.rt_normal,
            };
            let (command_tx, command_rx) = watch::channel(SourceCmd::SetTrigger(Some(src.trigger_provider)));
            source_command_senders_by_plugin
                .entry(src.plugin_name)
                .or_default()
                .push(command_tx);

            let task = run_source(src.name, src.source, data_tx, command_rx);
            source_set.spawn_on(task, runtime.handle());
        }

        // 4. Autonomous sources
        for src in self.autonomous_sources {
            let task = async move {
                src.source
                    .await
                    .map_err(|e| e.context(format!("error in autonomous source {}", src.name)))
            };
            source_set.spawn_on(task, self.rt_normal.handle());
        }

        // 5. Graceful shutdown and pipeline control.

        // mpsc channel for global shutdown order.
        let (global_shutdown_send, global_shutdown_recv) = mpsc::unbounded_channel::<()>();

        // Store the JoinSets to be able to wait for the tasks in a specific order (see pipeline_control_task).
        let join_sets = ElementJoinSets {
            source_set,
            transform_set,
            output_set,
        };

        // Spawn a task to control the pipeline and orchestrate its shutdown.
        // Most of the state (command senders, mask of the active transforms, etc.) is moved to this task.
        let (control_tx, control_rx) = mpsc::channel::<control::ControlMessage>(256);
        let controller_state = PipelineControllerState {
            global_shutdown_send,
            source_command_senders_by_plugin,
            output_command_senders_by_plugin,
            active_transforms,
            transforms_mask_by_plugin,
            autonomous_shutdown_token: self.autonomous_shutdown_token,
            modifier: PipelineModifierState {
                namegen: builder::ElementNameGenerator::new(),
                join_sets,
                in_tx,
                rt_normal: self.rt_normal.handle().clone(),
            },
        };
        let control_handle = control::ControlHandle { tx: control_tx };
        let control_task_handle = self.rt_normal.spawn(pipeline_control_task(
            global_shutdown_recv,
            control_rx,
            controller_state,
        ));

        RunningPipeline {
            _rt_normal: self.rt_normal,
            _rt_priority: self.rt_priority,
            shutdown_task_handle: Some(control_task_handle),
            control_handle,
        }
    }
}

/// Stores [`JoinSet`]s for all the tasks of the pipeline
/// that correspond to an element (source, transform, output).
struct ElementJoinSets {
    source_set: JoinSet<anyhow::Result<()>>,
    transform_set: JoinSet<anyhow::Result<()>>,
    output_set: JoinSet<anyhow::Result<()>>,
}

mod source {

}

mod transform {

}

mod output {

}

#[derive(Debug)]
pub struct PipelineError {
    pub error: anyhow::Error,
    pub element: ElementType,
}

impl From<PollError> for PipelineError {
    fn from(value: PollError) -> Self {
        Self {
            error: match value {
                PollError::Fatal(err) => err,
                PollError::CanRetry(err) => err,
            },
            element: ElementType::Source,
        }
    }
}

impl From<TransformError> for PipelineError {
    fn from(value: TransformError) -> Self {
        Self {
            error: match value {
                TransformError::Fatal(err) => err,
                TransformError::UnexpectedInput(err) => err,
            },
            element: ElementType::Transform,
        }
    }
}

impl From<WriteError> for PipelineError {
    fn from(value: WriteError) -> Self {
        Self {
            error: match value {
                WriteError::Fatal(err) => err,
                WriteError::CanRetry(err) => err,
            },
            element: ElementType::Output,
        }
    }
}

impl RunningPipeline {
    /// Blocks the current thread until all tasks in the pipeline finish.
    ///
    /// If a task returns an error or panicks, `wait_for_all` returns an error without waiting
    /// for the other tasks.
    pub fn wait_for_shutdown(mut self) -> anyhow::Result<()> {
        let handle = self.shutdown_task_handle.take().unwrap(); // cannot be called twice, unwrap should never panic
        let shutdown_res = self._rt_normal.block_on(async { handle.await });
        match shutdown_res {
            Ok(_) => Ok(()),
            Err(err) => {
                // task panicked or was cancelled
                if err.is_panic() {
                    log::error!("The shutdown task has panicked! {err:#}");
                } else if err.is_cancelled() {
                    log::error!("The shutdown task has been unexpectedly cancelled. {err:#}");
                }
                Err(err.into())
            }
        }
    }

    /// Returns a [`ControlHandle`], which allows to change the configuration
    /// of the pipeline while it is running.
    pub fn control_handle(&mut self) -> control::ControlHandle {
        self.control_handle.clone()
    }
}

impl Drop for RunningPipeline {
    fn drop(&mut self) {
        log::debug!("Dropping the pipeline...");
    }
}

impl ControlHandle {
    pub fn all(&self) -> ScopedControlHandle {
        ScopedControlHandle {
            handle: self,
            destination: MessageDestination::All,
        }
    }
    pub fn plugin(&self, plugin_name: impl Into<String>) -> ScopedControlHandle {
        ScopedControlHandle {
            handle: self,
            destination: MessageDestination::Plugin(plugin_name.into()),
        }
    }

    pub fn blocking_all(&self) -> BlockingScopedControlHandle {
        BlockingScopedControlHandle {
            handle: self,
            destination: MessageDestination::All,
        }
    }

    pub fn blocking_plugin(&self, plugin_name: impl Into<String>) -> BlockingScopedControlHandle {
        BlockingScopedControlHandle {
            handle: self,
            destination: MessageDestination::Plugin(plugin_name.into()),
        }
    }

    /// Requests the pipeline to shut down.
    pub fn shutdown(&self) {
        match self.tx.try_send(ControlMessage::Shutdown) {
            Ok(_) => {}
            Err(TrySendError::Closed(_)) => {
                // This may occur when the pipeline has already shut down. It's okay.
                log::debug!("ControlHandle::shutdown() has been called but the pipeline is already shutting down.")
            }
            Err(TrySendError::Full(_)) => {
                // TODO: handle this case? tx.blocking_send(ControlMessage::Shutdown) could work
                todo!("buffer is full, cannot send command")
            }
        }
    }

    /// Adds a new source to the pipeline, without interrupting the elements
    /// (sources, transforms, outputs) that are currently running.
    pub fn add_source(&self, plugin_name: String, source_name: String, source: Box<dyn Source>, trigger: TriggerSpec) {
        let msg = ControlMessage::AddSource {
            requested_name: source_name,
            plugin_name,
            source,
            trigger,
        };
        self.tx.try_send(msg).unwrap()
    }
}

pub struct ScopedControlHandle<'a> {
    handle: &'a ControlHandle,
    destination: MessageDestination,
}
impl<'a> ScopedControlHandle<'a> {
    pub async fn control_sources(self, command: SourceCmd) {
        // TODO investigate using try_send instead of send here
        self.handle
            .tx
            .send(ControlMessage::ModifySource(ElementCommand {
                destination: self.destination.clone(),
                command,
            }))
            .await
            .unwrap();
    }
    pub async fn control_transforms(self, command: TransformCmd) {
        self.handle
            .tx
            .send(ControlMessage::ModifyTransform(ElementCommand {
                destination: self.destination.clone(),
                command,
            }))
            .await
            .unwrap();
    }
    pub async fn control_outputs(self, command: OutputCmd) {
        self.handle
            .tx
            .send(ControlMessage::ModifyOutput(ElementCommand {
                destination: self.destination.clone(),
                command,
            }))
            .await
            .unwrap();
    }
}

pub struct BlockingScopedControlHandle<'a> {
    handle: &'a ControlHandle,
    destination: MessageDestination,
}
impl<'a> BlockingScopedControlHandle<'a> {
    pub fn control_sources(self, command: SourceCmd) {
        self.handle
            .tx
            .blocking_send(ControlMessage::ModifySource(ElementCommand {
                destination: self.destination.clone(),
                command,
            }))
            .unwrap();
    }
    pub fn control_transforms(self, command: TransformCmd) {
        self.handle
            .tx
            .blocking_send(ControlMessage::ModifyTransform(ElementCommand {
                destination: self.destination.clone(),
                command,
            }))
            .unwrap();
    }
    pub fn control_outputs(self, command: OutputCmd) {
        self.handle
            .tx
            .blocking_send(ControlMessage::ModifyOutput(ElementCommand {
                destination: self.destination.clone(),
                command,
            }))
            .unwrap();
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering},
            Arc,
        },
        thread::sleep,
        time::Duration,
    };

    use tokio::{
        runtime::Runtime,
        sync::{broadcast, mpsc, watch},
    };

    use crate::{
        measurement::{
            MeasurementAccumulator, MeasurementBuffer, MeasurementPoint, Timestamp, WrappedMeasurementType,
            WrappedMeasurementValue,
        },
        metrics::{MetricRegistry, RawMetricId},
        pipeline::{builder::ConfiguredTransform, trigger::TriggerSpec, OutputContext, Transform},
        resources::{Resource, ResourceConsumer},
    };

    use super::{
        super::trigger, run_output_from_broadcast, run_source, run_transforms, OutputCmd, OutputMsg, SourceCmd,
    };

    #[test]
    fn source_triggered_by_time_normal() {
        run_source_trigger_test(false);
    }
    #[test]
    fn source_triggered_by_time_with_interruption() {
        run_source_trigger_test(true);
    }

    fn run_source_trigger_test(with_interruption: bool) {
        let rt = new_rt(2);
        let source = TestSource::new();

        let period = Duration::from_millis(10);
        let flush_rounds = 3;
        let tp = new_trigger(with_interruption, period, flush_rounds);
        println!("trigger: {tp:?}");

        let (tx, mut rx) = mpsc::channel::<MeasurementBuffer>(64);
        let (cmd_tx, cmd_rx) = watch::channel(SourceCmd::SetTrigger(Some(tp)));

        let stopped = Arc::new(AtomicU8::new(TestSourceState::Running as _));
        let stopped2 = stopped.clone();
        rt.spawn(async move {
            let mut n_polls = 0;
            loop {
                // check the measurements
                if let Some(measurements) = rx.recv().await {
                    assert_ne!(
                        TestSourceState::Stopped as u8,
                        stopped2.load(Ordering::Relaxed),
                        "The source is stopped/paused, it should not produce measurements."
                    );

                    // Check that the correct number of rounds have passed.
                    assert_eq!(
                        measurements.len(),
                        flush_rounds,
                        "incorrect number of measurements: {measurements:?}"
                    );
                    n_polls += flush_rounds;

                    let last_point = measurements.iter().last().unwrap();
                    let last_point_value = match last_point.value {
                        WrappedMeasurementValue::U64(n) => n as usize,
                        _ => panic!("unexpected value type"),
                    };
                    assert_eq!(n_polls, last_point_value);
                } else {
                    // the channel is dropped when run_source terminates, which must only occur when the source is stopped
                    assert_ne!(
                        TestSourceState::Running as u8,
                        stopped2.load(Ordering::Relaxed),
                        "The source is not stopped, the channel should be open."
                    );
                }
            }
        });

        // poll the source for some time
        rt.spawn(run_source(String::from("test_source"), Box::new(source), tx, cmd_rx));
        sleep(2 * period);

        // pause source
        cmd_tx.send(SourceCmd::Pause).unwrap();
        stopped.store(TestSourceState::Stopping as _, Ordering::Relaxed);
        sleep(period); // some tolerance (wait for flushing)
        stopped.store(TestSourceState::Stopped as _, Ordering::Relaxed);

        // check that the source is paused
        sleep(2 * period);

        // still paused after SetTrigger
        cmd_tx
            .send(SourceCmd::SetTrigger(Some(new_trigger(
                with_interruption,
                period,
                flush_rounds,
            ))))
            .unwrap();
        sleep(2 * period);

        // resume source
        cmd_tx.send(SourceCmd::Run).unwrap();
        stopped.store(TestSourceState::Running as _, Ordering::Relaxed);
        sleep(period / 2); // lower tolerance (no flushing, just waiting for changes on the watch channel)

        // poll for some time
        sleep(period);

        // still running after SetTrigger
        cmd_tx
            .send(SourceCmd::SetTrigger(Some(new_trigger(
                with_interruption,
                period,
                flush_rounds,
            ))))
            .unwrap();
        sleep(2 * period);

        // stop source
        cmd_tx.send(SourceCmd::Stop).unwrap();
        stopped.store(TestSourceState::Stopping as _, Ordering::Relaxed);
        sleep(period); // some tolerance
        stopped.store(TestSourceState::Stopped as _, Ordering::Relaxed);

        // check that the source is stopped
        sleep(2 * period);

        // drop the runtime, abort the tasks
    }

    #[test]
    fn transform_task() {
        let rt = new_rt(2);

        // create transforms
        let check_input_type_for_transform3 = Arc::new(AtomicBool::new(true));
        let transforms: Vec<Box<dyn Transform>> = vec![
            Box::new(TestTransform {
                id: 1,
                expected_input_len: 2, // 2 because flush_interval = 2*poll_interval
                output_type: WrappedMeasurementType::U64,
                expected_input_type: WrappedMeasurementType::U64,
                check_input_type: Arc::new(AtomicBool::new(true)),
            }),
            Box::new(TestTransform {
                id: 2,
                expected_input_len: 2,
                output_type: WrappedMeasurementType::F64,
                expected_input_type: WrappedMeasurementType::U64,
                check_input_type: Arc::new(AtomicBool::new(true)),
            }),
            Box::new(TestTransform {
                id: 3,
                expected_input_len: 2,
                output_type: WrappedMeasurementType::F64,
                expected_input_type: WrappedMeasurementType::F64,
                check_input_type: check_input_type_for_transform3.clone(),
            }),
        ];
        let transforms: Vec<ConfiguredTransform> = transforms
            .into_iter()
            .map(|t| ConfiguredTransform {
                transform: t,
                name: String::from("test_transform"),
                plugin_name: String::from(""),
            })
            .collect();

        // create source
        let source = TestSource::new();
        let tp = new_trigger(false, Duration::from_millis(10), 2);
        let (src_tx, src_rx) = mpsc::channel::<MeasurementBuffer>(64);
        let (_src_cmd_tx, src_cmd_rx) = watch::channel(SourceCmd::SetTrigger(Some(tp)));

        // create transform channels and control flags
        let (trans_tx, mut out_rx) = broadcast::channel::<OutputMsg>(64);
        let active_flags = Arc::new(AtomicU64::new(u64::MAX));
        let active_flags2 = active_flags.clone();
        let active_flags3 = active_flags.clone();

        rt.spawn(async move {
            loop {
                if let Ok(OutputMsg::WriteMeasurements(measurements)) = out_rx.recv().await {
                    let current_flags = active_flags2.load(Ordering::Relaxed);
                    let transform1_enabled = current_flags & 1 != 0;
                    let transform2_enabled = current_flags & 2 != 0;
                    let transform3_enabled = current_flags & 4 != 0;
                    for m in measurements.iter() {
                        let int_val = match m.value {
                            WrappedMeasurementValue::F64(f) => f as u32,
                            WrappedMeasurementValue::U64(u) => u as u32,
                        };
                        if transform3_enabled {
                            assert_eq!(int_val, 3);
                            assert_eq!(m.value.measurement_type(), WrappedMeasurementType::F64);
                        } else if transform2_enabled {
                            assert_eq!(int_val, 2);
                            assert_eq!(m.value.measurement_type(), WrappedMeasurementType::F64);
                        } else if transform1_enabled {
                            assert_eq!(int_val, 1);
                            assert_eq!(m.value.measurement_type(), WrappedMeasurementType::U64);
                        } else {
                            assert_ne!(int_val, 3);
                            assert_ne!(int_val, 2);
                            assert_ne!(int_val, 1);
                            assert_eq!(m.value.measurement_type(), WrappedMeasurementType::U64);
                        }
                    }
                }
            }
        });

        // run the transforms
        rt.spawn(run_transforms(transforms, src_rx, trans_tx, active_flags3));

        // poll the source for some time
        rt.spawn(run_source(
            String::from("test_source"),
            Box::new(source),
            src_tx,
            src_cmd_rx,
        ));
        sleep(Duration::from_millis(20));

        // disable transform 3 only
        active_flags.store(1 | 2, Ordering::Relaxed);
        sleep(Duration::from_millis(20));

        // disable transform 1 only
        active_flags.store(2 | 4, Ordering::Relaxed);
        sleep(Duration::from_millis(20));

        // disable transform 2 only, the input type expected by transform 3 is no longer respected
        check_input_type_for_transform3.store(false, Ordering::Relaxed);
        active_flags.store(1 | 4, Ordering::Relaxed);
        sleep(Duration::from_millis(20));

        // disable all transforms
        active_flags.store(0, Ordering::Relaxed);
        check_input_type_for_transform3.store(true, Ordering::Relaxed);
        sleep(Duration::from_millis(20));

        // enable all transforms
        active_flags.store(1 | 2 | 4, Ordering::Relaxed);
        sleep(Duration::from_millis(20));
    }

    #[test]
    fn output_task() {
        let rt = new_rt(3);
        // create source
        let source = Box::new(TestSource::new());
        let tp = new_trigger(false, Duration::from_millis(10), 2);
        let (src_tx, trans_rx) = mpsc::channel::<MeasurementBuffer>(64);
        let (src_cmd_tx, src_cmd_rx) = watch::channel(SourceCmd::SetTrigger(Some(tp)));

        // no transforms but a transform task to send the values to the output
        let transforms = vec![];
        let (trans_tx, out_rx) = broadcast::channel::<OutputMsg>(64);
        let active_flags = Arc::new(AtomicU64::new(u64::MAX));

        // create output
        let output_count = Arc::new(AtomicU32::new(0));
        let output = Box::new(TestOutput {
            expected_input_len: 2,
            output_count: output_count.clone(),
        });
        let (out_cmd_tx, out_cmd_rx) = watch::channel(OutputCmd::Run);
        let out_ctx = OutputContext {
            metrics: MetricRegistry::new(),
        };

        // start tasks
        rt.spawn(run_output_from_broadcast(
            String::from("test_output"),
            output,
            out_rx,
            out_cmd_rx,
            out_ctx,
        ));
        rt.spawn(run_transforms(transforms, trans_rx, trans_tx, active_flags));
        rt.spawn(run_source(String::from("test_source"), source, src_tx, src_cmd_rx));

        // check the output
        sleep(Duration::from_millis(20));
        assert!(output_count.load(Ordering::Relaxed).abs_diff(4) <= 2);

        // pause and check
        out_cmd_tx.send(OutputCmd::Pause).unwrap();
        let count_at_pause = output_count.load(Ordering::Relaxed);
        sleep(Duration::from_millis(10));
        assert!(output_count.load(Ordering::Relaxed).abs_diff(count_at_pause) <= 2);
        sleep(Duration::from_millis(20));

        // resume and check
        let count_before_resume = output_count.load(Ordering::Relaxed);
        out_cmd_tx.send(OutputCmd::Run).unwrap();
        sleep(Duration::from_millis(20));
        assert!(output_count.load(Ordering::Relaxed) > count_before_resume);

        // stop and check
        src_cmd_tx.send(SourceCmd::Stop).unwrap();
        out_cmd_tx.send(OutputCmd::Stop).unwrap();
        sleep(Duration::from_millis(10));
        let count = output_count.load(Ordering::Relaxed);
        sleep(Duration::from_millis(20));
        assert_eq!(count, output_count.load(Ordering::Relaxed));
    }

    fn new_trigger(test_interrupt: bool, period: Duration, flush_rounds: usize) -> TriggerSpec {
        let mut builder = trigger::builder::time_interval(period)
            .flush_rounds(flush_rounds)
            .update_rounds(flush_rounds);

        if test_interrupt {
            builder = builder.update_interval(Duration::from_millis(1));
        }
        builder.build().unwrap()
    }

    fn new_rt(n_threads: usize) -> Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(n_threads)
            .enable_all()
            .build()
            .unwrap()
    }

    enum TestSourceState {
        /// The source must be running.
        Running = 0,
        /// The source has been stopped but can continue to run for some time
        /// before the next refresh of the commands (which occurs at the same time as flushing).
        Stopping = 1,
        /// The source has been stopped and should have been refreshed, no measurements must be produced.
        Stopped = 2,
    }

    struct TestSource {
        n_calls: u32,
    }
    impl TestSource {
        fn new() -> TestSource {
            TestSource { n_calls: 0 }
        }
    }
    impl crate::pipeline::Source for TestSource {
        fn poll(
            &mut self,
            into: &mut MeasurementAccumulator,
            timestamp: Timestamp,
        ) -> Result<(), crate::pipeline::PollError> {
            self.n_calls += 1;
            let point = MeasurementPoint::new_untyped(
                timestamp,
                RawMetricId(1),
                Resource::LocalMachine,
                ResourceConsumer::LocalMachine,
                WrappedMeasurementValue::U64(self.n_calls as u64),
            );
            into.push(point);
            Ok(())
        }
    }

    struct TestTransform {
        id: u32,
        output_type: WrappedMeasurementType,
        expected_input_len: usize,
        expected_input_type: WrappedMeasurementType,
        check_input_type: Arc<AtomicBool>,
    }

    impl crate::pipeline::Transform for TestTransform {
        fn apply(&mut self, measurements: &mut MeasurementBuffer) -> Result<(), crate::pipeline::TransformError> {
            assert_eq!(measurements.len(), self.expected_input_len);
            for m in measurements.iter_mut() {
                assert_eq!(m.resource, Resource::LocalMachine);
                if self.check_input_type.load(Ordering::Relaxed) {
                    assert_eq!(m.value.measurement_type(), self.expected_input_type);
                }
                m.value = match self.output_type {
                    WrappedMeasurementType::F64 => WrappedMeasurementValue::F64(self.id as _),
                    WrappedMeasurementType::U64 => WrappedMeasurementValue::U64(self.id as _),
                };
            }
            assert_eq!(measurements.len(), self.expected_input_len);
            Ok(())
        }
    }

    struct TestOutput {
        expected_input_len: usize,
        output_count: Arc<AtomicU32>,
    }

    impl crate::pipeline::Output for TestOutput {
        fn write(
            &mut self,
            measurements: &MeasurementBuffer,
            _ctx: &OutputContext,
        ) -> Result<(), crate::pipeline::WriteError> {
            assert_eq!(measurements.len(), self.expected_input_len);
            self.output_count.fetch_add(measurements.len() as _, Ordering::Relaxed);
            Ok(())
        }
    }
}
