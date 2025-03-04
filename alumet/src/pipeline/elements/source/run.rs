use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyhow::Context;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;

use crate::measurement::{MeasurementBuffer, Timestamp};
use crate::pipeline::naming::SourceName;
use crate::pipeline::trigger::TriggerReason;

use super::control::TaskState;
use super::error::PollError;
use super::interface::{AutonomousSource, Source};

pub(crate) async fn run_managed(
    source_name: SourceName,
    mut source: Box<dyn Source>,
    tx: mpsc::Sender<MeasurementBuffer>,
    config: Arc<super::task_controller::SharedSourceConfig>,
) -> anyhow::Result<()> {
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

    // main loop
    let mut i = 1usize;
    'run: loop {
        // Wait for the trigger. It can return for two reasons:
        // - "normal case": the underlying mechanism (e.g. timer) triggers <- this is the most likely case
        // - "interrupt case": the underlying mechanism was idle (e.g. sleeping) but a new command arrived
        let reason = trigger
            .next(config_change)
            .await
            .with_context(|| source_name.to_string())?;

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
                        return Err(e.context(format!("fatal error when polling {source_name}")));
                    }
                };

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

    Ok(())
}

pub async fn run_autonomous(source_name: SourceName, source: AutonomousSource) -> anyhow::Result<()> {
    source.await.map_err(|e| {
        log::error!("Error in autonomous source {source_name} (will stop running): {e:?}");
        e.context(format!("error in autonomous source {source_name}"))
    })
}
