//! Source triggers.

use std::time::Duration;
use std::{fmt, time};
use std::{future::Future, pin::Pin};

use tokio::sync::watch;

use super::runtime::SourceCmd;

/// A boxed future, from the `futures` crate.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The output of a SourceTrigger.
pub type SourceTriggerOutput = Result<(), std::io::Error>;

/// Defines a trigger for measurement sources.
///
/// The trigger controls when the [`Source`](super::Source) is polled for measurements.
#[derive(Debug, Clone)]
pub struct TriggerSpec {
    mechanism: TriggerMechanismSpec,
    interruptible: bool,
    config: TriggerConfig,
}

/// Controls when the [`Source`](super::Source) is polled for measurements.
pub(crate) struct Trigger {
    pub config: TriggerConfig,
    mechanism: TriggerMechanism,
    interrupt_signal: Option<watch::Receiver<SourceCmd>>,
}

#[derive(Debug, Clone)]
pub(crate) struct TriggerConfig {
    /// Numbers of polling operations to do before flushing the measurements.
    ///
    /// Flushing more often increases the pressure on the memory allocator.
    pub flush_rounds: usize,

    /// Number of polling operations to do before updating the command.
    ///
    /// Updating more often increases the overhead of the measurement,
    /// but decreases the time it takes for a [source command](super::runtime::SourceCmd)
    /// to be applied.
    pub update_rounds: usize,
}

/// Constraints that can be applied to a [`TriggerSpec`] after its construction.
pub(crate) struct TriggerConstraints {
    pub max_update_interval: time::Duration,
}

/// Builder for source triggers.
pub mod builder {
    use core::fmt;
    use std::time::{Duration, Instant};

    use super::{TriggerConfig, TriggerMechanismSpec, TriggerSpec};

    /// Returns a builder for a source trigger that polls the source at regular intervals.
    ///
    /// ## Timing
    ///
    /// The accuracy of the timing depends on the operating system and on the scheduling
    /// policy of the thread that executes the trigger.
    /// For small intervals of 1ms or less, it is recommended to run Alumet on Linux
    /// and to use [`SourceType::RealtimePriority`](super::runtime::SourceType::RealtimePriority).
    ///
    /// ## Example
    /// ```
    /// use alumet::pipeline::trigger;
    /// use std::time::{Instant, Duration};
    ///
    /// let trigger_config = trigger::builder::time_interval(Duration::from_secs(1))
    ///     .starting_at(Instant::now() + Duration::from_secs(30))
    ///     .flush_interval(Duration::from_secs(2))
    ///     .update_interval(Duration::from_secs(5))
    ///     .build()
    ///     .unwrap();
    /// ```
    pub fn time_interval(poll_interval: Duration) -> TimeTriggerBuilder {
        TimeTriggerBuilder::new(poll_interval)
    }

    /// Builder for a source trigger that polls the source at regular intervals.
    pub struct TimeTriggerBuilder {
        start: Instant,
        poll_interval: Duration,
        config: TriggerConfig,
        interruptible: bool,
    }

    #[derive(Debug)]
    pub enum Error {
        Io(std::io::Error),
        InvalidConfig(String),
    }

    impl fmt::Display for Error {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Error::Io(err) => write!(f, "io error: {err}"),
                Error::InvalidConfig(msg) => write!(f, "invalid trigger config: {msg}"),
            }
        }
    }

    impl std::error::Error for Error {}

    impl TimeTriggerBuilder {
        pub fn new(poll_interval: Duration) -> Self {
            Self {
                start: Instant::now(),
                poll_interval,
                config: TriggerConfig {
                    flush_rounds: 1,
                    update_rounds: 1,
                },
                interruptible: false,
            }
        }

        /// Start polling at the given time.
        pub fn starting_at(mut self, start: Instant) -> Self {
            self.start = start;
            self
        }

        /// Flush the measurements every `flush_rounds` polls.
        pub fn flush_rounds(mut self, flush_rounds: usize) -> Self {
            self.config.flush_rounds = flush_rounds;
            self
        }

        /// Update the source command every `update_rounds` polls.
        pub fn update_rounds(mut self, update_rounds: usize) -> Self {
            self.config.update_rounds = update_rounds;
            self
        }

        /// Flush the measurement after, at most, the given duration.
        pub fn flush_interval(mut self, flush_interval: Duration) -> Self {
            if self.poll_interval.is_zero() {
                return self; // don't modify anything, build() will fail
            }

            // flush_rounds must be non-zero, or the remainder operation will panic (`i % flush_rounds` in the polling loop)
            self.config.flush_rounds = ((flush_interval.as_nanos() / self.poll_interval.as_nanos()) as usize).max(1);
            self
        }

        /// Update the source command after, at most, the given duration.
        pub fn update_interval(mut self, update_interval: Duration) -> Self {
            if self.poll_interval.is_zero() {
                return self; // don't modify anything, build() will fail
            }

            if self.poll_interval > update_interval {
                // The trigger mechanism needs to be interruptible, otherwise `trigger.next().await`
                // would block the task for longer than the requested update interval, and
                // the source commands would be applied too late.
                self.config.update_rounds = 1;
                self.interruptible = true;
            } else {
                self.config.update_rounds =
                    ((update_interval.as_nanos() / self.poll_interval.as_nanos()) as usize).max(1);
                self.interruptible = false;
            }
            self
        }

        /// Builds the trigger.
        pub fn build(self) -> Result<TriggerSpec, Error> {
            if self.poll_interval.is_zero() {
                return Err(Error::InvalidConfig(format!("poll_interval must be non-zero")));
            }
            Ok(TriggerSpec {
                mechanism: TriggerMechanismSpec::TimeInterval(self.start, self.poll_interval),
                interruptible: self.interruptible,
                config: self.config,
            })
        }
    }
}

impl TriggerSpec {
    /// Defines a trigger that polls the source at regular intervals.
    ///
    /// For more options, use [`builder::time_interval`].
    pub fn at_interval(poll_interval: time::Duration) -> TriggerSpec {
        builder::time_interval(poll_interval).build().unwrap()
    }

    /// Adjusts the trigger specification to respect the given constraints.
    ///
    /// # Constraints
    /// - `max_update_interval`: maximum amount of time allowed between two command updates
    pub(crate) fn constrain(&mut self, constraints: &TriggerConstraints) {
        if !self.interruptible {
            let max_update_interval = constraints.max_update_interval;

            match self.mechanism {
                TriggerMechanismSpec::TimeInterval(_, poll_interval) => {
                    let update_interval = match self.config.update_rounds.try_into() {
                        Ok(update_rounds) => poll_interval * update_rounds,
                        Err(_too_big) => time::Duration::MAX,
                    };
                    if poll_interval > max_update_interval {
                        // The trigger mechanism needs to be interruptible to respect the max update time.
                        // See TimeTriggerBuilder::update_interval.
                        self.config.update_rounds = 1;
                        self.interruptible = true;
                    }
                    if update_interval > max_update_interval {
                        // Lower `update_rounds` to respect the max update time.
                        self.config.update_rounds =
                            ((max_update_interval.as_nanos() / poll_interval.as_nanos()) as usize).max(1);
                    }
                }
                _ => (),
            }
        }
    }
}

impl Default for TriggerConstraints {
    fn default() -> Self {
        Self {
            max_update_interval: Duration::MAX,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum TriggerReason {
    Triggered,
    Interrupted,
}

impl Trigger {
    pub fn new(spec: TriggerSpec, interrupt_signal: watch::Receiver<SourceCmd>) -> Result<Self, std::io::Error> {
        Ok(Self {
            config: spec.config,
            mechanism: TriggerMechanism::try_from(spec.mechanism)?,
            interrupt_signal: Some(interrupt_signal),
        })
    }

    #[allow(unused)]
    pub fn without_signal(spec: TriggerSpec) -> Result<Option<Self>, std::io::Error> {
        if spec.interruptible {
            Ok(None)
        } else {
            Ok(Some(Self {
                config: spec.config,
                mechanism: TriggerMechanism::try_from(spec.mechanism)?,
                interrupt_signal: None,
            }))
        }
    }

    pub async fn next(&mut self) -> anyhow::Result<TriggerReason> {
        if let Some(signal) = &mut self.interrupt_signal {
            // Use select! to wake up on trigger _or_ signal, the first that occurs
            tokio::select! {
                biased; // don't choose the branch randomly (for performance)

                res = self.mechanism.next() => {
                    res?;
                    Ok(TriggerReason::Triggered)
                }
                res = signal.changed() => {
                    res?;
                    Ok(TriggerReason::Interrupted)
                }
            }
        } else {
            // Simple case: simply wait for the trigger
            self.mechanism.next().await?;
            Ok(TriggerReason::Triggered)
        }
    }
}

/// Spec for a trigger mechanism.
///
/// Useful because some mechanisms, like tokio_timerfd::Interval, are not cloneable,
/// and we need cloneable values for working with the watch channel in
/// the implementation of the pipeline.
#[derive(Debug, Clone)]
enum TriggerMechanismSpec {
    TimeInterval(time::Instant, time::Duration),
    Future(fn() -> BoxFuture<'static, SourceTriggerOutput>),
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
    #[allow(dead_code)]
    TokioSleep(tokio::time::Instant, tokio::time::Duration),

    /// A trigger based on an arbitrary [`Future`] that is returned on demand
    /// by a function `f`.
    ///
    /// The source is polled each time `f().await` returns.
    Future(fn() -> BoxFuture<'static, SourceTriggerOutput>),
}

impl TryFrom<TriggerMechanismSpec> for TriggerMechanism {
    type Error = std::io::Error;

    fn try_from(value: TriggerMechanismSpec) -> Result<Self, Self::Error> {
        Ok(match value {
            TriggerMechanismSpec::TimeInterval(at, duration) => {
                // Use timerfd if possible, fallback to `tokio::time::sleep`.
                #[cfg(target_os = "linux")]
                {
                    TriggerMechanism::Timerfd(tokio_timerfd::Interval::new(at, duration)?)
                }

                #[cfg(not(target_os = "linux"))]
                {
                    TriggerMechanism::TokioSleep(at.into(), duration.into())
                }
            }
            TriggerMechanismSpec::Future(f) => TriggerMechanism::Future(f),
        })
    }
}

impl TriggerMechanism {
    pub async fn next(&mut self) -> Result<(), std::io::Error> {
        use tokio_stream::StreamExt;

        match self {
            #[cfg(target_os = "linux")]
            TriggerMechanism::Timerfd(interval) => {
                interval.next().await.unwrap()?;
                Ok(())
            }
            TriggerMechanism::TokioSleep(start, period) => {
                let now = tokio::time::Instant::now();
                let deadline = if &*start > &now { *start } else { now + *period };
                tokio::time::sleep_until(deadline).await;
                Ok(())
            }
            TriggerMechanism::Future(f) => f().await,
        }
    }
}

impl fmt::Debug for TriggerMechanism {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(target_os = "linux")]
            Self::Timerfd(_) => f.write_str("Timerfd trigger"),
            Self::TokioSleep(_, _) => f.write_str("TokioSleep trigger"),
            Self::Future(_) => f.write_str("Future trigger"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{builder, TriggerConstraints, TriggerMechanismSpec};

    #[test]
    fn trigger_auto_config() {
        let intervals_and_rounds = vec![
            (
                /*poll interval*/ 1,
                /*flush interval*/ 1,
                /*expected flush rounds or error*/ Some(1),
            ),
            (1, 2, Some(2)),
            (2, 1, Some(1)), // max(1)
            (2, 2, Some(1)),
            (22, 44, Some(2)),
            (21, 44, Some(2)), // rounding
            (22, 88, Some(4)),
            (0, 1, None),    // invalid interval
            (1, 1, Some(1)), // max(1)
            (1, 0, Some(1)), // max(1)
        ];
        for (poll_int, flush_int, expected_flush_rounds) in intervals_and_rounds {
            let res = builder::time_interval(Duration::from_secs(poll_int))
                .flush_interval(Duration::from_secs(flush_int))
                .build();
            match res {
                Ok(trigger_spec) => {
                    assert!(
                        expected_flush_rounds.is_some(),
                        "unexpected ok for ({}, {})",
                        poll_int,
                        flush_int
                    );
                    assert_eq!(expected_flush_rounds.unwrap(), trigger_spec.config.flush_rounds);
                }
                Err(_) => {
                    assert!(
                        expected_flush_rounds.is_none(),
                        "unexpected error for ({}, {})",
                        poll_int,
                        flush_int
                    );
                }
            }
        }
    }

    #[test]
    fn trigger_constraints() {
        let constraints = TriggerConstraints {
            max_update_interval: Duration::from_secs(2),
        };

        let mut trigger = builder::time_interval(Duration::from_secs(1)) // 1sec
            .flush_interval(Duration::from_secs(5)) // 5*1sec
            .update_interval(Duration::from_secs(2)) // 2*1sec
            .build()
            .unwrap();
        trigger.constrain(&constraints);
        assert!(matches!(trigger.mechanism, TriggerMechanismSpec::TimeInterval(_, d) if d == Duration::from_secs(1)));
        assert_eq!(trigger.config.flush_rounds, 5); // 5*1sec => 5 rounds
        assert_eq!(trigger.config.update_rounds, 2); // 2*1sec => 2rounds
        
        let mut trigger = builder::time_interval(Duration::from_secs(2))
            .flush_interval(Duration::from_secs(10))
            .update_interval(Duration::from_secs(2))
            .build()
            .unwrap();
        trigger.constrain(&constraints);
        assert!(matches!(trigger.mechanism, TriggerMechanismSpec::TimeInterval(_, d) if d == Duration::from_secs(2)));
        assert_eq!(trigger.config.flush_rounds, 5);
        assert_eq!(trigger.config.update_rounds, 1);
        
        let mut trigger = builder::time_interval(Duration::from_secs(2))
            .flush_interval(Duration::from_secs(10))
            .update_interval(Duration::from_secs(6)) // multiple of poll_interval
            .build()
            .unwrap();
        trigger.constrain(&constraints);
        assert!(matches!(trigger.mechanism, TriggerMechanismSpec::TimeInterval(_, d) if d == Duration::from_secs(2)));
        assert_eq!(trigger.config.flush_rounds, 5);
        assert_eq!(trigger.config.update_rounds, 1);
        
        let mut trigger = builder::time_interval(Duration::from_secs(2))
            .flush_interval(Duration::from_secs(10))
            .update_interval(Duration::from_secs(5)) // not a multiple of poll_interval!
            .build()
            .unwrap();
        trigger.constrain(&constraints);
        assert!(matches!(trigger.mechanism, TriggerMechanismSpec::TimeInterval(_, d) if d == Duration::from_secs(2)));
        assert_eq!(trigger.config.flush_rounds, 5);
        assert_eq!(trigger.config.update_rounds, 1);
        
        let mut trigger = builder::time_interval(Duration::from_secs(3)) // bigger than max_update_interval!
            .flush_interval(Duration::from_secs(15))
            .update_interval(Duration::from_secs(3))
            .build()
            .unwrap();
        trigger.constrain(&constraints);
        assert!(matches!(trigger.mechanism, TriggerMechanismSpec::TimeInterval(_, d) if d == Duration::from_secs(3)));
        assert!(trigger.interruptible);
        assert_eq!(trigger.config.flush_rounds, 5);
        assert_eq!(trigger.config.update_rounds, 1);
    }
}
