use alumet::{
    measurement::MeasurementBuffer,
    metrics::RawMetricId,
    pipeline::{
        Transform,
        elements::{error::TransformError, transform::TransformContext},
    },
};
use std::collections::HashSet;

enum FilterMode {
    Include(HashSet<RawMetricId>),
    Exclude(HashSet<RawMetricId>),
}

pub struct FilterTransform {
    mode: FilterMode,
}

impl FilterTransform {
    pub fn new(include: Option<HashSet<RawMetricId>>, exclude: Option<HashSet<RawMetricId>>) -> anyhow::Result<Self> {
        let mode = match (include, exclude) {
            (Some(ids), None) => FilterMode::Include(ids),
            (None, Some(ids)) => FilterMode::Exclude(ids),
            (None, None) => {
                return Err(anyhow::anyhow!(
                    "filter transform cannot have both include and exclude empty"
                ));
            } // shouldn't happen as it's validated at plugin init stage
            (Some(_), Some(_)) => {
                return Err(anyhow::anyhow!(
                    "filter transform cannot have both include and exclude set"
                ));
            } // shouldn't happen as it's validated at plugin init stage
        };

        Ok(Self { mode })
    }
}

impl Transform for FilterTransform {
    fn apply(&mut self, measurements: &mut MeasurementBuffer, _ctx: &TransformContext) -> Result<(), TransformError> {
        match &self.mode {
            FilterMode::Include(set) => {
                measurements.retain(|p| set.contains(&p.metric));
            }

            FilterMode::Exclude(set) => {
                measurements.retain(|p| !set.contains(&p.metric));
            }
        }

        Ok(())
    }
}
