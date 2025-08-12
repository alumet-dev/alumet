use core::fmt;
use std::time::{Duration, Instant};

use super::{TriggerLoopParams, TriggerMechanismSpec, TriggerSpec};

/// Returns a builder for a source trigger spec that polls the source at regular intervals.
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
/// use alumet::pipeline::elements::source::trigger;
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

/// Returns a builder for a source trigger spec that polls the source when "manually" requested.
pub fn manual() -> ManualTriggerBuilder {
    ManualTriggerBuilder::new()
}

struct TriggerSpecBuilder {
    mechanism: TriggerMechanismSpec,
    loop_params: TriggerLoopParams,
    interruptible: bool,
    manual_allowed: bool,
    realtime_sched_priority: bool,
}

/// Builder for a trigger that wakes up at regular intervals.
pub struct TimeTriggerBuilder(TriggerSpecBuilder);

/// Builder for a trigger that only wakes up on "manual" notifications.
pub struct ManualTriggerBuilder(TriggerSpecBuilder);

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

impl TriggerSpecBuilder {
    fn new(mechanism: TriggerMechanismSpec) -> Self {
        Self {
            mechanism,
            loop_params: TriggerLoopParams {
                flush_rounds: 1,
                update_rounds: 1,
            },
            interruptible: false,
            manual_allowed: false,
            realtime_sched_priority: false,
        }
    }

    fn build(&mut self) -> TriggerSpec {
        TriggerSpec {
            mechanism: self.mechanism.clone(),
            interruptible: self.interruptible,
            allow_manual_trigger: self.manual_allowed,
            use_realtime_priority: self.realtime_sched_priority,
            loop_params: self.loop_params.clone(),
        }
    }

    /// Flush the measurements every `flush_rounds` polls.
    fn flush_rounds(&mut self, flush_rounds: usize) {
        if flush_rounds == 0 {
            panic!("flush_rounds must be non-zero");
        }
        self.loop_params.flush_rounds = flush_rounds;
    }

    /// Update the source command every `update_rounds` polls.
    fn update_rounds(&mut self, update_rounds: usize) {
        if update_rounds == 0 {
            panic!("update_rounds must be non-zero");
        }
        self.loop_params.update_rounds = update_rounds;
    }
}

impl TimeTriggerBuilder {
    pub fn new(poll_interval: Duration) -> Self {
        Self(TriggerSpecBuilder::new(TriggerMechanismSpec::TimeInterval(
            Instant::now(),
            poll_interval,
        )))
    }

    fn poll_interval_mut(&mut self) -> &mut Duration {
        match &mut self.0.mechanism {
            TriggerMechanismSpec::TimeInterval(_, duration) => duration,
            _ => unreachable!(),
        }
    }

    fn poll_interval(&self) -> &Duration {
        match &self.0.mechanism {
            TriggerMechanismSpec::TimeInterval(_, duration) => duration,
            _ => unreachable!(),
        }
    }

    /// Start polling at the given time.
    pub fn starting_at(&mut self, start: Instant) -> &mut Self {
        match &mut self.0.mechanism {
            TriggerMechanismSpec::TimeInterval(instant, _) => *instant = start,
            _ => unreachable!(),
        }
        self
    }

    /// Flush the measurements every `flush_rounds` polls.
    pub fn flush_rounds(&mut self, flush_rounds: usize) -> &mut Self {
        self.0.flush_rounds(flush_rounds);
        self
    }

    /// Update the source command every `update_rounds` polls.
    pub fn update_rounds(&mut self, update_rounds: usize) -> &mut Self {
        self.0.update_rounds(update_rounds);
        self
    }

    /// Flush the measurement after, at most, the given duration.
    pub fn flush_interval(&mut self, flush_interval: Duration) -> &mut Self {
        if self.poll_interval().is_zero() {
            return self; // don't modify anything, build() will fail
        }

        // flush_rounds must be non-zero, or the remainder operation will panic (`i % flush_rounds` in the polling loop)
        self.0.loop_params.flush_rounds =
            ((flush_interval.as_nanos() / self.poll_interval().as_nanos()) as usize).max(1);
        self
    }

    /// Update the source command after, at most, the given duration.
    pub fn update_interval(&mut self, update_interval: Duration) -> &mut Self {
        let poll_interval = *self.poll_interval();
        if poll_interval.is_zero() {
            return self; // don't modify anything, build() will fail
        }

        if poll_interval > update_interval {
            // The trigger mechanism needs to be interruptible, otherwise `trigger.next().await`
            // would block the task for longer than the requested update interval, and
            // the source commands would be applied too late.
            self.0.loop_params.update_rounds = 1;
            self.0.interruptible = true;
        } else {
            self.0.loop_params.update_rounds =
                ((update_interval.as_nanos() / poll_interval.as_nanos()) as usize).max(1);
            self.0.interruptible = false;
        }
        self
    }

    /// Signals that the pipeline should run the source on a thread with a high scheduling priority.
    ///
    /// The actual implementation of this "high priority" is OS-dependent and comes with no strong guarantee.
    /// On Linux, it typically means calling `sched_setscheduler` to change the scheduler priority.
    ///
    /// Note that Alumet may decide to apply this setting automatically for high polling frequencies (low `poll_interval`).
    pub fn realtime_priority(&mut self) -> &mut Self {
        self.0.realtime_sched_priority = true;
        self
    }

    pub fn allow_manual_trigger(&mut self) -> &mut Self {
        self.0.manual_allowed = true;
        self
    }

    /// Builds the trigger specification.
    pub fn build(&mut self) -> Result<TriggerSpec, Error> {
        let poll_interval = *self.poll_interval();
        if poll_interval.is_zero() {
            return Err(Error::InvalidConfig(String::from("poll_interval must be non-zero")));
        }

        // automatically enable `realtime_priority` in some cases
        // TODO make this configurable
        if poll_interval <= Duration::from_millis(3) {
            self.0.realtime_sched_priority = true;
        }

        Ok(self.0.build())
    }
}

impl ManualTriggerBuilder {
    pub fn new() -> Self {
        let mut inner = TriggerSpecBuilder::new(TriggerMechanismSpec::ManualOnly);
        // Make it interruptible by default, otherwise config updates will only be applied when
        // manually triggered.
        inner.interruptible = true;
        Self(inner)
    }

    pub fn interruptible(&mut self, interruptible: bool) -> &mut Self {
        self.0.interruptible = interruptible;
        self
    }

    /// Flush the measurements every `flush_rounds` polls.
    pub fn flush_rounds(&mut self, flush_rounds: usize) -> &mut Self {
        self.0.flush_rounds(flush_rounds);
        self
    }

    /// Update the source command every `update_rounds` polls.
    pub fn update_rounds(&mut self, update_rounds: usize) -> &mut Self {
        self.0.update_rounds(update_rounds);
        self
    }

    /// Builds the trigger specification.
    pub fn build(&mut self) -> Result<TriggerSpec, Error> {
        Ok(self.0.build())
    }
}
