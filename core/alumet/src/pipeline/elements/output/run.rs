use std::{
    ops::ControlFlow,
    sync::{Arc, Mutex, atomic::Ordering},
};

use crate::{
    measurement::MeasurementBuffer,
    metrics::online::MetricReader,
    pipeline::{
        error::PipelineError,
        naming::OutputName,
        util::channel::{self, RecvError},
    },
};

use super::{BoxedAsyncOutput, Output, OutputContext, control, error::WriteError};

pub async fn run_async_output(name: OutputName, output: BoxedAsyncOutput) -> Result<(), PipelineError> {
    output.await.map_err(|e| {
        log::error!("Error when asynchronously writing to {name} (will stop running): {e:?}");
        PipelineError::for_element(name, e)
    })
}

pub async fn run_blocking_output<Rx: channel::MeasurementReceiver>(
    name: OutputName,
    guarded_output: Arc<Mutex<Box<dyn Output>>>,
    mut rx: Rx,
    metrics_reader: MetricReader,
    config: Arc<control::SharedOutputConfig>,
) -> Result<(), PipelineError> {
    /// If `measurements` is an `Ok`, build an [`OutputContext`] and call `output.write(&measurements, &ctx)`.
    /// Otherwise, handle the error.
    async fn write_measurements(
        name: &OutputName,
        output: Arc<Mutex<Box<dyn Output>>>,
        metrics_r: MetricReader,
        maybe_measurements: Result<MeasurementBuffer, channel::RecvError>,
    ) -> anyhow::Result<ControlFlow<()>> {
        match maybe_measurements {
            Ok(measurements) => {
                log::trace!("writing {} measurements to {name}", measurements.len());
                let res = tokio::task::spawn_blocking(move || {
                    let ctx = OutputContext {
                        metrics: &metrics_r.blocking_read(),
                    };
                    output.lock().unwrap().write(&measurements, &ctx)
                })
                .await?;
                match res {
                    Ok(()) => Ok(ControlFlow::Continue(())),
                    Err(WriteError::CanRetry(e)) => {
                        log::error!("Non-fatal error when writing to {name} (will retry): {e:#}");
                        Ok(ControlFlow::Continue(()))
                    }
                    Err(WriteError::Fatal(e)) => {
                        log::error!("Fatal error when writing to {name} (will stop running): {e:?}");
                        Err(e.context(format!("fatal error when writing to {name}")))
                    }
                }
            }
            Err(channel::RecvError::Lagged(n)) => {
                log::warn!("Output {name} is too slow, it lost the oldest {n} messages.");
                Ok(ControlFlow::Continue(()))
            }
            Err(channel::RecvError::Closed) => {
                log::debug!("The channel connected to output {name} was closed, it will now stop.");
                Ok(ControlFlow::Break(()))
            }
        }
    }

    let config_change = &config.change_notifier;
    let mut receive = true;
    let mut finish = false;
    loop {
        tokio::select! {
            _ = config_change.notified() => {
                let new_state = config.atomic_state.load(Ordering::Relaxed);
                match new_state.into() {
                    control::TaskState::Run => {
                        receive = true;
                    }
                    control::TaskState::RunDiscard => {
                        // Resume the output but discard the data that is in the buffer.
                        // The output will only see the measurements that are sent after this point.
                        rx = rx.discard_pending();
                        receive = true;
                    }
                    control::TaskState::Pause => {
                        receive = false;
                    }
                    control::TaskState::StopNow => {
                        break; // stop the output and ignore the remaining data
                    }
                    control::TaskState::StopFinish => {
                        finish = true;
                        break; // stop the output and empty the channel
                    }
                }
            },
            measurements = rx.recv(), if receive => {
                let res = write_measurements(&name, guarded_output.clone(), metrics_reader.clone(), measurements)
                    .await
                    .map_err(|e| PipelineError::for_element(name.clone(), e))?;
                if res.is_break() {
                    finish = false; // just in case
                    break
                }
            }
        }
    }

    if finish {
        // Write the last measurements, ignore any lag (the latter is done in write_measurements).
        // This is useful when Alumet is stopped, to ensure that we don't discard any data.
        loop {
            log::trace!("{name} finishing...");
            let received = rx.recv().await;
            log::trace!(
                "{name} finishing with {}",
                match &received {
                    Ok(buf) => format!("Ok(buf of size {})", buf.len()),
                    Err(RecvError::Closed) => String::from("Err(Closed)"),
                    Err(RecvError::Lagged(n)) => format!("Err(Lagged({n}))"),
                }
            );
            let res = write_measurements(&name, guarded_output.clone(), metrics_reader.clone(), received)
                .await
                .map_err(|e| PipelineError::for_element(name.clone(), e))?;
            if res.is_break() {
                break;
            }
        }
    }

    Ok(())
}
