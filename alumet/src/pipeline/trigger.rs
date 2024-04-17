//! Source triggers.

use std::time::{Duration, Instant};
use std::{fmt, io};
use std::{future::Future, pin::Pin};

use super::PollError;

/// A boxed future, from the `futures` crate.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
/// The output of a SourceTrigger.
pub type SourceTriggerOutput = Result<(), PollError>;

/// Provides a `SourceTrigger` on demand, for `Source`s.
#[derive(Clone, Debug)]
pub enum TriggerProvider {
    /// A trigger provider based on a precise time interval.  This is much more
    /// accurate than [`std::thread::sleep`] and [`tokio::time::sleep`],
    /// but is only available on Linux.
    TimeInterval {
        /// Time of the first polling.
        start_time: Instant,
        /// Time interval between each polling.
        poll_interval: Duration,
        /// Time interval between each flushing of the measurements.
        flush_interval: Duration,
    },
    /// A trigger based on an arbitrary [`Future`] that is returned on demand
    /// by a function `f`.
    Future {
        /// Function that creates a (boxed) Future.
        f: fn() -> BoxFuture<'static, SourceTriggerOutput>,
        /// How many calls to the function `f` should be made before flushing the measurements.
        flush_rounds: usize,
    },
}

/// Controls when the [`Source`](super::Source) is polled for measurements.
pub(crate) struct SourceTrigger {
    /// The trigger that controls when to poll the source.
    trigger: TriggerMechanism,
    /// Numbers of polling operations to do before flushing the measurements.
    pub flush_rounds: usize,
}

/// The possible trigger mechanisms.
enum TriggerMechanism {
    /// A trigger based on a precise time interval. This is much more
    /// accurate than [`std::thread::sleep`] and [`tokio::time::sleep`],
    /// but is only available on Linux.
    ///
    /// The source is polled each time `interval.next().await` returns.
    #[cfg(target_os = "linux")]
    Timerfd(tokio_timerfd::Interval),

    /// A trigger based on [`tokio::time::sleep`].
    TokioSleep(tokio::time::Instant, tokio::time::Duration),

    /// A trigger based on an arbitrary [`Future`] that is returned on demand
    /// by a function `f`.
    ///
    /// The source is polled each time `f().await` returns.
    Future(fn() -> BoxFuture<'static, SourceTriggerOutput>),
}

impl SourceTrigger {
    pub async fn next(&mut self) -> Result<(), PollError> {
        use tokio_stream::StreamExt;

        match self.trigger {
            #[cfg(target_os = "linux")]
            TriggerMechanism::Timerfd(ref mut interval) => {
                interval.next().await.unwrap()?;
                Ok(())
            }
            TriggerMechanism::TokioSleep(start, period) => {
                let now = tokio::time::Instant::now();
                let deadline = if start > now { start } else { now + period };
                tokio::time::sleep_until(deadline).await;
                Ok(())
            }
            TriggerMechanism::Future(f) => f().await,
        }
    }
}

impl TriggerProvider {
    /// Returns a new `SourceTrigger` along with some automatic settings.
    pub(crate) fn auto_configured(self) -> io::Result<SourceTrigger> {
        match self {
            TriggerProvider::TimeInterval {
                start_time,
                poll_interval,
                flush_interval,
            } => {
                if flush_interval.is_zero() || poll_interval.is_zero() || flush_interval < poll_interval {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "Invalid intervals, they must be non-zero, and poll_interval must be >= flush_interval.",
                    ));
                }
                // flush_rounds must be non-zero, or the remainder operation will panic (`i % flush_rounds` in the polling loop)
                let flush_rounds = (flush_interval.as_micros() / poll_interval.as_micros()) as usize;

                // Use timerfd if possible, fallback to `tokio::time::sleep`.
                #[cfg(target_os = "linux")]
                let trigger = TriggerMechanism::Timerfd(tokio_timerfd::Interval::new(start_time, poll_interval)?);
                #[cfg(not(target_os = "linux"))]
                let trigger = TriggerMechanism::TokioSleep(tokio::time::Instant::from_std(start_time), poll_interval);

                Ok(SourceTrigger { trigger, flush_rounds })
            }
            TriggerProvider::Future { f, flush_rounds } => {
                let trigger = TriggerMechanism::Future(f);
                Ok(SourceTrigger { trigger, flush_rounds })
            }
        }
    }
}

impl fmt::Debug for TriggerMechanism {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(target_os = "linux")]
            Self::Timerfd(_) => f.write_str("TimerfdInterval"),
            Self::TokioSleep(_, _) => f.write_str("TokioSleep"),
            Self::Future(_) => f.write_str("Future"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::TriggerProvider;

    #[test]
    fn trigger_auto_config() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let intervals_and_rounds = vec![
            (
                /*poll interval*/ 1,
                /*flush interval*/ 1,
                /*expected flush rounds or error*/ Some(1),
            ),
            (1, 2, Some(2)),
            (2, 1, None), // flushing more often than polling is impossible!
            (2, 2, Some(1)),
            (22, 44, Some(2)),
            (21, 44, Some(2)), // rounding
            (22, 88, Some(4)),
            (0, 1, None), // invalid interval
            (1, 0, None), // invalid interval
            (0, 0, None), // invalid interval
        ];
        for (poll_int, flush_int, expected_flush_rounds) in intervals_and_rounds {
            let tp = TriggerProvider::TimeInterval {
                start_time: Instant::now(),
                poll_interval: Duration::from_secs(poll_int),
                flush_interval: Duration::from_secs(flush_int),
            };
            rt.block_on(async move {
                match tp.auto_configured() {
                    Ok(trigger) => {
                        assert!(expected_flush_rounds.is_some());
                        assert_eq!(expected_flush_rounds.unwrap(), trigger.flush_rounds);
                    }
                    Err(_) => {
                        assert!(expected_flush_rounds.is_none());
                    }
                }
            });
        }
    }
}
