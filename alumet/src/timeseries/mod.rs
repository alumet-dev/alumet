use crate::measurement::{MeasurementBuffer, MeasurementPoint};

pub struct TimeseriesProcessor {}

pub struct ProcessedTimeseries {}

pub struct GroupKey {}
pub struct Group {}
pub struct Interpolation {}

impl TimeseriesProcessor {
    pub fn process(m: &mut MeasurementBuffer) -> ProcessedTimeseries {
        todo!()
    }
}

impl ProcessedTimeseries {
    pub fn groups(&self) -> impl Iterator<Item = (GroupKey, Group)> {
        todo!()
    }

    pub fn synchronize_on(&self, main_pace: GroupKey, interp: Interpolation) {
        todo!()
    }
}

impl Group {
    pub fn iter(&self) -> impl Iterator<Item = &Vec<&MeasurementPoint>> {
        todo!()
    }
}

mod attempt2 {
    pub struct Timeseries;
    pub struct GroupedTimeseries;
    pub struct SynchronizedTimeseries;

    impl Timeseries {
        pub fn group_by(self) -> GroupedTimeseries {
            todo!()
        }
        
        
    }
    
    impl GroupedTimeseries {
        pub fn synchronize_on(self) -> SynchronizedTimeseries {
            todo!()
        }
    }
}
