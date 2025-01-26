//! Source triggers.

use std::sync::Arc;
use std::time::Duration;
use std::{fmt, time};
use std::{future::Future, pin::Pin};

use tokio::sync::Notify;

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
    allow_manual_trigger: bool,
    use_realtime_priority: bool,
    config: TriggerConfig,
}

/// Controls when the [`Source`](super::Source) is polled for measurements.
pub(crate) struct Trigger {
    pub config: TriggerConfig,
    inner: TriggerImpl,
}

enum TriggerImpl {
    Simple(TriggerMechanism),
    Interruptible(TriggerMechanism),
    WithManualTrigger(TriggerMechanism, bool, Arc<Notify>),
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
pub struct TriggerConstraints {
    /// Sets the maximum interval between two updates of the commands processed by
    /// each measurement [`Source`](crate::pipeline::Source).
    ///
    /// This only applies to the sources that are triggered by a time interval
    /// managed by Alumet, i.e. the "managed sources".
    pub max_update_interval: time::Duration,

    /// If `true`, forces all managed sources to be triggered on-demand by a signal.
    pub allow_manual_trigger: bool,
}

/// Builder for source triggers.
///
/// See [`builder::time_interval`].
pub mod builder {
    use core::fmt;
    use std::time::{Duration, Instant};

    use super::{TriggerConfig, TriggerMechanismSpec, TriggerSpec};

    /// Returns a builder for a source trigger that polls the source at regular intervals.
    ///
    /// # Timing
    ///
    /// The accuracy of the timing depends on the operating system and on the scheduling
    /// policy of the thread that executes the trigger.
    /// For small intervals of 1ms or less, it is recommended to run Alumet on Linux
    /// and to use the high "realtime" scheduling priority by calling [`TimeTriggerBuilder::realtime_priority`].
    ///
    /// # Example
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
        manual_trigger: bool,
        realtime_priority: bool,
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
                manual_trigger: false,
                realtime_priority: false,
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

        /// Signals that the pipeline should run the source on a thread with a high scheduling priority.
        ///
        /// The actual implementation of this "high priority" is OS-dependent and comes with no strong guarantee.
        /// On Linux, it typically means calling `sched_setscheduler` to change the scheduler priority.
        ///
        /// Note that Alumet may decide to apply this setting automatically for high polling frequencies (low `poll_interval`).
        pub fn realtime_priority(mut self) -> Self {
            self.realtime_priority = true;
            self
        }

        pub fn allow_manual_trigger(mut self) -> Self {
            self.manual_trigger = true;
            self
        }

        /// Builds the trigger.
        pub fn build(mut self) -> Result<TriggerSpec, Error> {
            if self.poll_interval.is_zero() {
                return Err(Error::InvalidConfig(String::from("poll_interval must be non-zero")));
            }
            // automatically enable `realtime_priority` in some cases
            if self.poll_interval <= Duration::from_millis(3) {
                self.realtime_priority = true;
            }

            Ok(TriggerSpec {
                mechanism: TriggerMechanismSpec::TimeInterval(self.start, self.poll_interval),
                interruptible: self.interruptible,
                allow_manual_trigger: self.manual_trigger,
                use_realtime_priority: self.realtime_priority,
                config: self.config,
            })
        }
    }
}

pub(crate) mod private_impl {
    use super::TriggerSpec;

    impl PartialEq for TriggerSpec {
        fn eq(&self, other: &Self) -> bool {
            match (&self.mechanism, &other.mechanism) {
                (
                    super::TriggerMechanismSpec::TimeInterval(_, duration_a),
                    super::TriggerMechanismSpec::TimeInterval(_, duration_b),
                ) => duration_a == duration_b,
                (super::TriggerMechanismSpec::Future(_f1), super::TriggerMechanismSpec::Future(_f2)) => {
                    true // how to std::ptr::eq on this?
                }
                _ => false,
            }
        }
    }
    impl Eq for TriggerSpec {}
}

impl TriggerSpec {
    /// Defines a trigger that polls the source at regular intervals.
    ///
    /// For more options, use [`builder::time_interval`].
    pub fn at_interval(poll_interval: time::Duration) -> TriggerSpec {
        builder::time_interval(poll_interval).build().unwrap()
    }

    /// Creates a new builder for a trigger that polls the source at regular intervals.
    ///
    /// This is equivalent to [`builder::time_interval`].
    pub fn builder(poll_interval: time::Duration) -> builder::TimeTriggerBuilder {
        builder::time_interval(poll_interval)
    }

    /// Adjusts the trigger specification to respect the given constraints.
    ///
    /// # Constraints
    /// - `max_update_interval`: maximum amount of time allowed between two command updates
    pub(crate) fn constrain(&mut self, constraints: &TriggerConstraints) {
        if constraints.allow_manual_trigger {
            self.allow_manual_trigger = constraints.allow_manual_trigger;
        }
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

    pub(crate) fn requests_realtime_priority(&self) -> bool {
        self.use_realtime_priority
    }
}

impl Default for TriggerConstraints {
    fn default() -> Self {
        Self {
            max_update_interval: Duration::MAX,
            allow_manual_trigger: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum TriggerReason {
    Triggered,
    Interrupted,
}

pub struct ManualTrigger(Arc<Notify>);

impl ManualTrigger {
    pub fn trigger_now(&self) {
        self.0.notify_one();
    }
}

impl Trigger {
    pub fn new(spec: TriggerSpec) -> Result<Self, std::io::Error> {
        let mechanism = TriggerMechanism::try_from(spec.mechanism)?;
        let inner = if spec.allow_manual_trigger {
            TriggerImpl::WithManualTrigger(mechanism, spec.interruptible, Arc::new(Notify::new()))
        } else if spec.interruptible {
            TriggerImpl::Interruptible(mechanism)
        } else {
            TriggerImpl::Simple(mechanism)
        };
        Ok(Self {
            config: spec.config,
            inner,
        })
    }

    pub fn manual_trigger(&self) -> Option<ManualTrigger> {
        match &self.inner {
            TriggerImpl::WithManualTrigger(_, _, notify) => Some(ManualTrigger(notify.clone())),
            _ => None,
        }
    }

    /// Waits for the next tick of the trigger, or for an interruption (if enabled).
    pub async fn next(&mut self, interrupt: &Notify) -> anyhow::Result<TriggerReason> {
        match &mut self.inner {
            TriggerImpl::Simple(mechanism) => {
                // Simple case: wait for the trigger to wake up
                mechanism.next().await?;
                Ok(TriggerReason::Triggered)
            }
            TriggerImpl::Interruptible(mechanism) => {
                // wait for the first of two futures: normal trigger or "interruption"
                tokio::select! {
                    biased; // don't choose the branch randomly (for performance)

                    res = mechanism.next() => {
                        res?;
                        Ok(TriggerReason::Triggered)
                    },
                    _ = interrupt.notified() => {
                        Ok(TriggerReason::Interrupted)
                    }
                }
            }
            TriggerImpl::WithManualTrigger(mechanism, interruptible, manual_trigger) => {
                tokio::select! {
                    biased;

                    res = mechanism.next() => {
                        res?;
                        Ok(TriggerReason::Triggered)
                    },
                    _ = interrupt.notified(), if *interruptible => {
                        Ok(TriggerReason::Interrupted)
                    }
                    _ = manual_trigger.notified() => {
                        Ok(TriggerReason::Triggered)
                    }
                }
            }
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
    #[allow(dead_code)]
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
                let start = *start;
                let now = tokio::time::Instant::now();
                let deadline = if start > now { start } else { now + *period };
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
            allow_manual_trigger: false,
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
