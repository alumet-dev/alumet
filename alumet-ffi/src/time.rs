use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ====== Timestamp ======
#[repr(C)]
pub struct Timestamp {
    secs: u64,
    nanos: u32,
}

impl From<SystemTime> for Timestamp {
    fn from(value: SystemTime) -> Self {
        let diff = value
            .duration_since(UNIX_EPOCH)
            .expect("Every timestamp should be obtained from system_time_now()");
        Timestamp {
            secs: diff.as_secs(),
            nanos: diff.subsec_nanos(),
        }
    }
}

impl From<Timestamp> for SystemTime {
    fn from(value: Timestamp) -> Self {
        UNIX_EPOCH + Duration::new(value.secs, value.nanos)
    }
}

impl From<Timestamp> for alumet::measurement::Timestamp {
    fn from(value: Timestamp) -> Self {
        alumet::measurement::Timestamp::from(SystemTime::from(value))
    }
}

impl From<alumet::measurement::Timestamp> for Timestamp {
    fn from(value: alumet::measurement::Timestamp) -> Self {
        Timestamp::from(SystemTime::from(value))
    }
}

// ====== Duration ======
#[repr(C)]
pub struct TimeDuration {
    pub t: Timestamp,
}

impl From<Duration> for TimeDuration {
    fn from(value: Duration) -> Self {
        Self {
            t: Timestamp {
                secs: value.as_secs(),
                nanos: value.subsec_nanos(),
            },
        }
    }
}

impl From<TimeDuration> for Duration {
    fn from(value: TimeDuration) -> Self {
        Duration::new(value.t.secs, value.t.nanos)
    }
}
