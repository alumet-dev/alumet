//! Rate computation from successive raw counter samples.

use alumet::measurement::Timestamp;

use crate::sysfs::RawCounters;

/// Size, in bytes, of one unit of the InfiniBand data counters.
///
/// `port_rcv_data`/`port_xmit_data` count "data octets divided by 4" (IBTA spec,
/// Linux sysfs ABI, `perfquery(8)`), so the divisor belongs to the counter
/// definition and is not the link width. See README, "Sysfs counters".
const DATA_COUNTER_UNIT_BYTES: f64 = 4.0;

/// One raw counter sample, with the time at which it was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    pub timestamp: Timestamp,
    pub counters: RawCounters,
}

/// Per-port rates, in packets per second and bytes per second.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rates {
    pub rcv_packets_per_s: f64,
    pub xmit_packets_per_s: f64,
    pub rcv_bytes_per_s: f64,
    pub xmit_bytes_per_s: f64,
}

/// Computes rates between two samples.
///
/// Returns `None` if the elapsed time is not strictly positive, or if any
/// counter decreased (which indicates a reset or a wrap-around).
pub fn compute_rates(prev: &Sample, cur: &Sample) -> Option<Rates> {
    let elapsed = cur.timestamp.duration_since(prev.timestamp).ok()?.as_secs_f64();
    if elapsed <= 0.0 {
        return None;
    }

    let rcv_packets = cur.counters.rcv_packets.checked_sub(prev.counters.rcv_packets)?;
    let xmit_packets = cur.counters.xmit_packets.checked_sub(prev.counters.xmit_packets)?;
    let rcv_data = cur.counters.rcv_data.checked_sub(prev.counters.rcv_data)?;
    let xmit_data = cur.counters.xmit_data.checked_sub(prev.counters.xmit_data)?;

    Some(Rates {
        rcv_packets_per_s: rcv_packets as f64 / elapsed,
        xmit_packets_per_s: xmit_packets as f64 / elapsed,
        rcv_bytes_per_s: (rcv_data as f64) * DATA_COUNTER_UNIT_BYTES / elapsed,
        xmit_bytes_per_s: (xmit_data as f64) * DATA_COUNTER_UNIT_BYTES / elapsed,
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn sample(secs: u64, counters: RawCounters) -> Sample {
        Sample {
            timestamp: Timestamp::from_unix_timestamp(secs, 0),
            counters,
        }
    }

    fn counters(rcv_packets: u64, xmit_packets: u64, rcv_data: u64, xmit_data: u64) -> RawCounters {
        RawCounters {
            rcv_packets,
            xmit_packets,
            rcv_data,
            xmit_data,
        }
    }

    /// Asserts that two rates are equal up to floating-point rounding.
    #[track_caller]
    fn assert_rate(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-9, "expected {expected}, got {actual}");
    }

    #[test]
    fn computes_packet_rates() {
        let prev = sample(0, counters(0, 0, 0, 0));
        let cur = sample(2, counters(1_000_000, 2_000_000, 0, 0));
        let rates = compute_rates(&prev, &cur).unwrap();
        assert_rate(rates.rcv_packets_per_s, 500_000.0);
        assert_rate(rates.xmit_packets_per_s, 1_000_000.0);
    }

    #[test]
    fn converts_data_units_to_bytes() {
        let prev = sample(0, counters(0, 0, 0, 0));
        // 250 units of 4 bytes per second = 1000 B/s.
        let cur = sample(1, counters(0, 0, 250, 500));
        let rates = compute_rates(&prev, &cur).unwrap();
        assert_rate(rates.rcv_bytes_per_s, 1000.0);
        assert_rate(rates.xmit_bytes_per_s, 2000.0);
    }

    #[test]
    fn decreasing_counter_is_rejected() {
        let prev = sample(0, counters(100, 100, 100, 100));
        let cur = sample(1, counters(99, 100, 100, 100));
        assert!(compute_rates(&prev, &cur).is_none());
    }

    #[test]
    fn zero_elapsed_is_rejected() {
        let prev = sample(0, counters(0, 0, 0, 0));
        let cur = sample(0, counters(10, 10, 10, 10));
        assert!(compute_rates(&prev, &cur).is_none());
    }

    #[test]
    fn backwards_clock_is_rejected() {
        let prev = sample(10, counters(0, 0, 0, 0));
        let cur = Sample {
            timestamp: prev.timestamp - Duration::from_secs(1),
            counters: counters(10, 10, 10, 10),
        };
        assert!(compute_rates(&prev, &cur).is_none());
    }
}
