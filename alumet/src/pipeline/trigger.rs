use std::time::{Duration, Instant};
use std::{fmt, io};
use std::{future::Future, pin::Pin};

use super::PollError;

/// A boxed future, from the `futures` crate.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
/// The output of a SourceTrigger.
pub type SourceTriggerOutput = Result<(), PollError>;

/// A trigger controls when the [`Source`] is polled for measurements.
pub enum SourceTrigger {
    /// A trigger based on a precise time interval. This is much more
    /// accurate than [`std::thread::sleep`] and [`tokio::time::sleep`],
    /// but is only available on Linux.
    /// 
    /// The source is polled each time `interval.next().await` returns.
    TimeInterval(tokio_timerfd::Interval),

    /// A trigger based on an arbitrary [`Future`] that is returned on demand
    /// by a function `f`.
    /// 
    /// The source is polled each time `f().await` returns.
    Future(fn() -> BoxFuture<'static, SourceTriggerOutput>),
}

/// A trigger + some settings.
pub(crate) struct ConfiguredTrigger {
    /// The trigger that controls when to poll the source.
    pub trigger: SourceTrigger,
    /// Numbers of polling operations to do before flushing the measurements.
    pub flush_rounds: usize,
}

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
impl TriggerProvider {
    /// Returns a new `SourceTrigger` along with some automatic settings.
    pub(crate) fn auto_configured(self) -> io::Result<ConfiguredTrigger> {
        match self {
            TriggerProvider::TimeInterval {
                start_time,
                poll_interval,
                flush_interval,
            } => {
                let flush_rounds = (flush_interval.as_micros() / poll_interval.as_micros()) as usize;
                let trigger = SourceTrigger::TimeInterval(tokio_timerfd::Interval::new(start_time, poll_interval)?);
                Ok(ConfiguredTrigger { trigger, flush_rounds })
            }
            TriggerProvider::Future { f, flush_rounds } => {
                let trigger = SourceTrigger::Future(f);
                Ok(ConfiguredTrigger { trigger, flush_rounds })
            }
        }
    }
}

impl fmt::Debug for SourceTrigger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TimeInterval(_) => f.write_str("TimeInterval"),
            Self::Future(_) => f.write_str("Future"),
        }
    }
}
