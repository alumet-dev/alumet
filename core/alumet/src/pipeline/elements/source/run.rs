use std::sync::Arc;
use std::sync::atomic::Ordering;

use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;

use crate::measurement::{MeasurementBuffer, Timestamp};
use crate::pipeline::error::PipelineError;
use crate::pipeline::naming::SourceName;
use crate::pipeline::util::coop::TriggerCoop;

use super::control::TaskState;
use super::error::PollError;
use super::interface::{AutonomousSource, Source};
use super::trigger::TriggerReason;

pub(crate) async fn run_managed(
    source_name: SourceName,
    mut source: Box<dyn Source>,
    tx: mpsc::Sender<MeasurementBuffer>,
    config: Arc<super::task_controller::SharedSourceConfig>,
) -> Result<(), PipelineError> {
    /// Flushes the measurement and returns a new buffer.
    async fn flush(
        buffer: MeasurementBuffer,
        tx: &mpsc::Sender<MeasurementBuffer>,
        name: &SourceName,
    ) -> MeasurementBuffer {
        // Hint for the new buffer capacity, great if the number of measurements per flush doesn't change much,
        // which is often the case.
        let prev_length = buffer.len();

        match tx.try_send(buffer) {
            Ok(()) => {
                // buffer has been sent, create a new one
                log::debug!("{name} flushed {prev_length} measurements");
                MeasurementBuffer::with_capacity(prev_length)
            }
            Err(TrySendError::Closed(_buf)) => {
                // the channel Receiver has been closed
                panic!("source channel should stay open");
            }
            Err(TrySendError::Full(buffer)) => {
                // the channel's buffer is full! reduce the measurement frequency
                // TODO it would be better to choose which source to slow down based
                // on its frequency and number of measurements per poll.
                // buf
                log::warn!(
                    "The buffer [sources -> transforms] is full! Consider increasing poll_interval for some sources"
                );
                let t0 = Timestamp::now();
                tx.send(buffer).await.expect("source channel should stay open");
                let t1 = Timestamp::now();
                let delta = t1.duration_since(t0).unwrap();
                log::debug!(
                    "{name} flushed {prev_length} measurements, after waiting {} µs",
                    delta.as_micros()
                );
                MeasurementBuffer::with_capacity(prev_length)
            }
        }
    }

    // Estimate the required buffer capacity with the new trigger and allocate it.
    fn adapt_buffer_after_trigger_change(
        buffer: &mut MeasurementBuffer,
        prev_flush_rounds: usize,
        new_flush_rounds: usize,
    ) {
        let prev_length = buffer.len();
        let remaining_rounds = new_flush_rounds;
        let hint_additional_elems = remaining_rounds * prev_length / prev_flush_rounds;
        buffer.reserve(hint_additional_elems);
    }

    // Get the initial source configuration.
    let init_trigger = config
        .take_new_trigger()
        .expect("the Trigger must be set before starting the source");
    log::trace!("{source_name} got initial config");

    // Store measurements in this buffer, and replace it every `flush_rounds` rounds.
    // For now, we don't know how many measurements the source will produce, so we allocate 1 per round.
    let mut buffer = MeasurementBuffer::with_capacity(init_trigger.params.flush_rounds);

    // This Notify is used to "interrupt" the trigger mechanism when the source configuration is modified
    // by the control loop.
    let config_change = &config.change_notifier;

    // Cooperate nicely with other tasks (avoid starvation).
    let mut coop = TriggerCoop::new();
    let mut trigger = init_trigger;

    let mut run = false;
    while !run {
        let initial_state = config.atomic_state.load(Ordering::Relaxed);
        log::trace!("{source_name} initial state: {initial_state}");
        match initial_state.into() {
            TaskState::Run => {
                run = true; // start the main loop
            }
            TaskState::Pause => {
                let pause_timeout = tokio::time::Duration::from_secs(60); // todo: make it configurable
                if let Err(_) = tokio::time::timeout(pause_timeout, config_change.notified()).await {
                    log::info!(
                        "Source {source_name} has been started in Pause state and not be resumed in {:?} - Stopping it",
                        pause_timeout
                    );
                    return Ok(());
                }
            }
            TaskState::Stop => {
                log::warn!("Source {source_name} has been started in Stop state and will stop immediately.");
                return Ok(());
            }
        }
    }

    // main loop
    let mut i = 1usize;
    'run: loop {
        // Wait for the trigger. It can return for two reasons:
        // - "normal case": the underlying mechanism (e.g. timer) triggers <- this is the most likely case
        // - "interrupt case": the underlying mechanism was idle (e.g. sleeping) but a new command arrived
        log::trace!("{source_name} waiting for next trigger");
        let reason = coop
            .with_budget(trigger.next(config_change))
            .await
            .map_err(|err| PipelineError::for_element(source_name.clone(), err))?;

        log::trace!("{source_name} triggered with reason {reason:?}");

        let mut update;
        match reason {
            TriggerReason::Triggered => {
                // poll the source
                let timestamp = Timestamp::now();
                match source.poll(&mut buffer.as_accumulator(), timestamp) {
                    Ok(()) => (),
                    Err(PollError::NormalStop) => {
                        log::info!("Source {source_name} stopped itself.");
                        break 'run; // stop polling
                    }
                    Err(PollError::CanRetry(e)) => {
                        log::error!("Non-fatal error when polling {source_name} (will retry): {e:#}");
                    }
                    Err(PollError::Fatal(e)) => {
                        log::error!("Fatal error when polling {source_name} (will stop running): {e:?}");
                        return Err(PipelineError::for_element(source_name, e));
                    }
                };

                // Flush the measurements, not on every round for performance reasons.
                // This is done _after_ polling, to ensure that we poll at least once before flushing, even if flush_rounds is 1.
                if i % trigger.params.flush_rounds == 0 {
                    // flush and create a new buffer
                    buffer = flush(buffer, &tx, &source_name).await;
                }

                // only update on some rounds, for performance reasons.
                update = (i % trigger.params.update_rounds) == 0;
                i = i.wrapping_add(1);
            }
            TriggerReason::Interrupted => {
                // interrupted because of a new command, forcibly update the command (see below)
                update = true;
            }
        };
        log::trace!("{source_name} update = {update}");

        while update {
            let new_state = config.atomic_state.load(Ordering::Relaxed);
            log::trace!("{source_name} new state: {new_state:?}");
            if let Some(new_trigger) = config.take_new_trigger() {
                // adapt the buffer size
                let prev_flush_rounds = trigger.params.flush_rounds;
                let new_flush_rounds = new_trigger.params.flush_rounds;
                adapt_buffer_after_trigger_change(&mut buffer, prev_flush_rounds, new_flush_rounds);
                // use the new trigger
                trigger = new_trigger;
            }
            match new_state.into() {
                TaskState::Run => {
                    update = false; // go back to polling
                }
                TaskState::Pause => {
                    log::trace!("{source_name} has been paused; resetting its internal state");
                    if let Err(e) = source.reset() {
                        log::error!("Failed to reset {source_name}: {e:?}");
                    };
                    log::trace!("{source_name} is now waiting to restart or stop permanently");
                    config_change.notified().await; // wait for the config to change
                }
                TaskState::Stop => {
                    break 'run; // stop polling
                }
            }
        }
    }

    // source stopped, flush the buffer
    log::debug!("{source_name} is stopping...");
    if !buffer.is_empty() {
        flush(buffer, &tx, &source_name).await;
    }

    // log the name of the source, so we know which source terminates
    log::debug!("{source_name} stops.");
    Ok(())
}

pub async fn run_autonomous(source_name: SourceName, source: AutonomousSource) -> Result<(), PipelineError> {
    match source.await {
        Ok(_) => {
            log::debug!("{source_name} stops.");
            Ok(())
        }
        Err(e) => {
            log::error!("Error in autonomous source {source_name} (will stop running): {e:?}");
            Err(PipelineError::for_element(source_name, e))
        }
    }
}
