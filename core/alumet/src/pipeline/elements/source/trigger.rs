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
    loop_params: TriggerLoopParams,
}

/// Controls when the [`Source`](super::Source) is polled for measurements.
pub(crate) struct Trigger {
    pub config: TriggerLoopParams,
    inner: TriggerImpl,
}

enum TriggerImpl {
    /// Single-mechanism trigger, potentially interruptible.
    Single(TriggerMechanism, Interruptible),
    /// Dual-mechanism trigger: triggers on the first mechanism that awakes.
    Double(TriggerMechanism, TriggerMechanism, Interruptible),
}

enum Interruptible {
    Yes,
    No,
}

#[derive(Debug, Clone)]
pub(crate) struct TriggerLoopParams {
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
pub mod builder;

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
            self.allow_manual_trigger = true;
        }
        if !self.interruptible {
            let max_update_interval = constraints.max_update_interval;

            match self.mechanism {
                TriggerMechanismSpec::TimeInterval(_, poll_interval) => {
                    let update_interval = match self.loop_params.update_rounds.try_into() {
                        Ok(update_rounds) => poll_interval * update_rounds,
                        Err(_too_big) => time::Duration::MAX,
                    };
                    if poll_interval > max_update_interval {
                        // The trigger mechanism needs to be interruptible to respect the max update time.
                        // See TimeTriggerBuilder::update_interval.
                        self.loop_params.update_rounds = 1;
                        self.interruptible = true;
                    }
                    if update_interval > max_update_interval {
                        // Lower `update_rounds` to respect the max update time.
                        self.loop_params.update_rounds =
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

impl From<bool> for Interruptible {
    fn from(value: bool) -> Self {
        match value {
            true => Interruptible::Yes,
            false => Interruptible::No,
        }
    }
}

impl Trigger {
    pub fn new(spec: TriggerSpec) -> Result<Self, std::io::Error> {
        let interruptible = Interruptible::from(spec.interruptible);
        let manual_only = matches!(spec.mechanism, TriggerMechanismSpec::ManualOnly);
        let mechanism = TriggerMechanism::try_from(spec.mechanism)?;
        let inner = if spec.allow_manual_trigger && !manual_only {
            let manual = TriggerMechanism::Manual(Arc::new(Notify::new()));
            TriggerImpl::Double(mechanism, manual, interruptible)
        } else {
            TriggerImpl::Single(mechanism, interruptible)
        };
        Ok(Self {
            config: spec.loop_params,
            inner,
        })
    }

    pub fn manual_trigger(&self) -> Option<ManualTrigger> {
        match &self.inner {
            TriggerImpl::Single(TriggerMechanism::Manual(notify), _)
            | TriggerImpl::Double(TriggerMechanism::Manual(notify), _, _)
            | TriggerImpl::Double(_, TriggerMechanism::Manual(notify), _) => Some(ManualTrigger(notify.clone())),
            _ => None,
        }
    }

    /// Waits for the next tick of the trigger, or for an interruption (if enabled).
    pub async fn next(&mut self, interrupt: &Notify) -> anyhow::Result<TriggerReason> {
        match &mut self.inner {
            TriggerImpl::Single(mechanism, Interruptible::No) => {
                // Simple case: wait for the trigger to wake up
                mechanism.next().await?;
                Ok(TriggerReason::Triggered)
            }
            TriggerImpl::Single(mechanism, Interruptible::Yes) => {
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
            TriggerImpl::Double(m1, m2, interruptible) => {
                tokio::select! {
                    biased;

                    res = m1.next() => {
                        res?;
                        Ok(TriggerReason::Triggered)
                    },
                    res = m2.next() => {
                        res?;
                        Ok(TriggerReason::Triggered)
                    }
                    _ = interrupt.notified(), if matches!(interruptible, Interruptible::Yes) => {
                        Ok(TriggerReason::Interrupted)
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
    #[allow(unused)]
    Future(fn() -> BoxFuture<'static, SourceTriggerOutput>),
    ManualOnly,
}

/// A mechanism that can trigger things.
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
    Sleep(tokio::time::Instant, tokio::time::Duration),

    /// A "manual" trigger based on [`tokio::sync::Notify`].
    Manual(Arc<Notify>),

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
                    TriggerMechanism::Sleep(at.into(), duration.into())
                }
            }
            TriggerMechanismSpec::Future(f) => TriggerMechanism::Future(f),
            TriggerMechanismSpec::ManualOnly => TriggerMechanism::Manual(Arc::new(Notify::new())),
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
            TriggerMechanism::Sleep(start, period) => {
                let start = *start;
                let now = tokio::time::Instant::now();
                let deadline = if start > now { start } else { now + *period };
                tokio::time::sleep_until(deadline).await;
                Ok(())
            }
            TriggerMechanism::Future(f) => f().await,
            TriggerMechanism::Manual(notify) => Ok(notify.notified().await),
        }
    }
}

impl fmt::Debug for TriggerMechanism {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(target_os = "linux")]
            Self::Timerfd(_) => f.write_str("TriggerMechanism::Timerfd"),
            Self::Sleep(_, _) => f.write_str("TriggerMechanism::Sleep"),
            Self::Future(ptr) => write!(f, "TriggerMechanism::Future({ptr:?})"),
            Self::Manual(_) => f.write_str("TriggerMechanism::Manual"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{TriggerConstraints, TriggerMechanismSpec, builder};

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
                    assert_eq!(expected_flush_rounds.unwrap(), trigger_spec.loop_params.flush_rounds);
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
        assert_eq!(trigger.loop_params.flush_rounds, 5); // 5*1sec => 5 rounds
        assert_eq!(trigger.loop_params.update_rounds, 2); // 2*1sec => 2rounds

        let mut trigger = builder::time_interval(Duration::from_secs(2))
            .flush_interval(Duration::from_secs(10))
            .update_interval(Duration::from_secs(2))
            .build()
            .unwrap();
        trigger.constrain(&constraints);
        assert!(matches!(trigger.mechanism, TriggerMechanismSpec::TimeInterval(_, d) if d == Duration::from_secs(2)));
        assert_eq!(trigger.loop_params.flush_rounds, 5);
        assert_eq!(trigger.loop_params.update_rounds, 1);

        let mut trigger = builder::time_interval(Duration::from_secs(2))
            .flush_interval(Duration::from_secs(10))
            .update_interval(Duration::from_secs(6)) // multiple of poll_interval
            .build()
            .unwrap();
        trigger.constrain(&constraints);
        assert!(matches!(trigger.mechanism, TriggerMechanismSpec::TimeInterval(_, d) if d == Duration::from_secs(2)));
        assert_eq!(trigger.loop_params.flush_rounds, 5);
        assert_eq!(trigger.loop_params.update_rounds, 1);

        let mut trigger = builder::time_interval(Duration::from_secs(2))
            .flush_interval(Duration::from_secs(10))
            .update_interval(Duration::from_secs(5)) // not a multiple of poll_interval!
            .build()
            .unwrap();
        trigger.constrain(&constraints);
        assert!(matches!(trigger.mechanism, TriggerMechanismSpec::TimeInterval(_, d) if d == Duration::from_secs(2)));
        assert_eq!(trigger.loop_params.flush_rounds, 5);
        assert_eq!(trigger.loop_params.update_rounds, 1);

        let mut trigger = builder::time_interval(Duration::from_secs(3)) // bigger than max_update_interval!
            .flush_interval(Duration::from_secs(15))
            .update_interval(Duration::from_secs(3))
            .build()
            .unwrap();
        trigger.constrain(&constraints);
        assert!(matches!(trigger.mechanism, TriggerMechanismSpec::TimeInterval(_, d) if d == Duration::from_secs(3)));
        assert!(trigger.interruptible);
        assert_eq!(trigger.loop_params.flush_rounds, 5);
        assert_eq!(trigger.loop_params.update_rounds, 1);
    }
}
