use std::ops::RangeInclusive;

use crate::measurement::{MeasurementBuffer, MeasurementPoint, Timestamp};

pub mod grouped_buffer;
pub mod interpolate;
pub mod multi_interp;
pub mod together;

#[derive(Default)]
pub struct Timeseries {
    // **sorted** (by timestamp) points
    points: Vec<MeasurementPoint>,
}

pub struct Timeslice<'a> {
    points: &'a [MeasurementPoint],
}

impl Timeseries {
    pub fn first(&self) -> Option<&MeasurementPoint> {
        self.points.first()
    }

    pub fn last(&self) -> Option<&MeasurementPoint> {
        self.points.last()
    }

    pub fn as_slice(&self) -> Timeslice {
        Timeslice { points: &self.points }
    }
}

impl<'a> Timeslice<'a> {
    pub fn restrict(&self, range: RangeInclusive<Timestamp>) -> Timeslice<'a> {
        // the data points are sorted, we just need to find the borders
        let i_first_ok = self
            .points
            .iter()
            .enumerate()
            .find_map(|(i, m)| if &m.timestamp >= range.start() { Some(i) } else { None });
        let i_last_ok = self
            .points
            .iter()
            .rev()
            .enumerate()
            .find_map(|(i, m)| if &m.timestamp <= range.end() { Some(i) } else { None });
        if let (Some(first), Some(last)) = (i_first_ok, i_last_ok) {
            if last > first {
                return Timeslice {
                    points: &self.points[first..=last],
                };
            }
        }
        // nothing in range
        Timeslice {
            points: &self.points[0..0],
        }
    }
}

impl From<MeasurementBuffer> for Timeseries {
    fn from(value: MeasurementBuffer) -> Self {
        let mut points: Vec<MeasurementPoint> = value.into_iter().collect();
        points.sort_by_key(|p| p.timestamp);
        Self { points }
    }
}

impl From<Vec<MeasurementPoint>> for Timeseries {
    fn from(mut points: Vec<MeasurementPoint>) -> Self {
        points.sort_by_key(|p| p.timestamp);
        Self { points }
    }
}

impl<'a> From<&'a [MeasurementPoint]> for Timeslice<'a> {
    fn from(points: &'a [MeasurementPoint]) -> Self {
        assert!(points.is_sorted_by_key(|p| p.timestamp));
        Self { points }
    }
}
