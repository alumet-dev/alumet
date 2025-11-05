use std::sync::Arc;
use std::sync::atomic::Ordering;

use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;

use crate::measurement::{MeasurementBuffer, Timestamp};
use crate::pipeline::error::PipelineError;
use crate::pipeline::naming::SourceName;

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
    fn flush(buffer: MeasurementBuffer, tx: &mpsc::Sender<MeasurementBuffer>, name: &SourceName) -> MeasurementBuffer {
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
            Err(TrySendError::Full(_buf)) => {
                // the channel's buffer is full! reduce the measurement frequency
                // TODO it would be better to choose which source to slow down based
                // on its frequency and number of measurements per poll.
                // buf
                todo!("buffer is full")
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
    let mut trigger = config
        .new_trigger
        .lock()
        .unwrap()
        .take()
        .expect("the Trigger must be set before starting the source");
    log::trace!("source {source_name} got initial config");

    // Store measurements in this buffer, and replace it every `flush_rounds` rounds.
    // For now, we don't know how many measurements the source will produce, so we allocate 1 per round.
    let mut buffer = MeasurementBuffer::with_capacity(trigger.config.flush_rounds);

    // This Notify is used to "interrupt" the trigger mechanism when the source configuration is modified
    // by the control loop.
    let config_change = &config.change_notifier;

    let mut run = false;
    while !run {
        let initial_state = config.atomic_state.load(Ordering::Relaxed);
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
        let reason = trigger
            .next(config_change)
            .await
            .map_err(|err| PipelineError::for_element(source_name.clone(), err))?;

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
                if log::log_enabled!(log::Level::Debug) {
                    let end = Timestamp::now();
                    let delta = end.duration_since(timestamp).unwrap();
                    log::debug!("source {source_name} polled in {} µs", delta.as_micros());
                }

                // Flush the measurements, not on every round for performance reasons.
                // This is done _after_ polling, to ensure that we poll at least once before flushing, even if flush_rounds is 1.
                if i % trigger.config.flush_rounds == 0 {
                    // flush and create a new buffer
                    buffer = flush(buffer, &tx, &source_name);
                }

                // only update on some rounds, for performance reasons.
                update = (i % trigger.config.update_rounds) == 0;
                i = i.wrapping_add(1);
            }
            TriggerReason::Interrupted => {
                // interrupted because of a new command, forcibly update the command (see below)
                update = true;
            }
        };

        while update {
            let new_state = config.atomic_state.load(Ordering::Relaxed);
            let new_trigger = config.new_trigger.lock().unwrap().take();
            if let Some(t) = new_trigger {
                let prev_flush_rounds = trigger.config.flush_rounds;
                let new_flush_rounds = t.config.flush_rounds;
                trigger = t;
                adapt_buffer_after_trigger_change(&mut buffer, prev_flush_rounds, new_flush_rounds);
            }
            match new_state.into() {
                TaskState::Run => {
                    update = false; // go back to polling
                }
                TaskState::Pause => {
                    config_change.notified().await; // wait for the config to change
                }
                TaskState::Stop => {
                    break 'run; // stop polling
                }
            }
        }
    }

    // source stopped, flush the buffer
    if !buffer.is_empty() {
        flush(buffer, &tx, &source_name);
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
