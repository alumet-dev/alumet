use std::{
    collections::HashMap, sync::{Arc, Mutex}, time::{SystemTime, UNIX_EPOCH}
};

use alumet::{
    measurement::{AttributeValue, MeasurementBuffer, MeasurementPoint, WrappedMeasurementValue}, metrics::{online::MetricReader, registry::MetricRegistry, RawMetricId}, pipeline::{
        elements::{error::TransformError, transform::TransformContext},
        Transform,
    }, resources::Resource
};
use anyhow::anyhow;

pub struct EnergyAttributionTransform {
    pub metrics: super::Metrics,
    consumed_energy_buffer: HashMap<u128, MeasurementPoint>,
    global_hardware_usage_buffer: HashMap<u128, MeasurementPoint>,
    hardware_usage_buffer: HashMap<u128, Vec<MeasurementPoint>>,
    hardware_usage_metric_filter: HashMap<String, String>,

    metric_reader: Arc<Mutex<Option<MetricRegistry>>>,
}

impl EnergyAttributionTransform {
    /// Instantiates a new EnergyAttributionTransform with its private fields initialized.
    pub fn new(metrics: super::Metrics, hardware_usage_metric_filter: HashMap<String, String>,  metric_reader: &Arc<Mutex<Option<MetricRegistry>>>) -> Self {
        Self {
            metrics,
            hardware_usage_buffer: HashMap::<u128, Vec<MeasurementPoint>>::new(),
            consumed_energy_buffer: HashMap::<u128, MeasurementPoint>::new(),
            global_hardware_usage_buffer: HashMap::<u128, MeasurementPoint>::new(),
            hardware_usage_metric_filter: hardware_usage_metric_filter,
            metric_reader: metric_reader.clone()
        }
    }

    /// Empties the buffers and send the energy attribution points to the MeasurementBuffer.
    fn buffer_bouncer(&mut self, measurements: &mut alumet::measurement::MeasurementBuffer) {
        // Retrieving the metric_id of the energy attribution.
        // Using a nested scope to reduce the lock time.
        let metric_id = self.metrics.attributed_energy;

        // If the buffers do have enough (every) MeasurementPoints,
        // then we compute the energy attribution.
        while self.consumed_energy_buffer.len() >= 2
            && self.hardware_usage_buffer.len() >= 2
            && self.global_hardware_usage_buffer.len() >= 2
        {
            // Get the smallest consumed_energy id i.e. the oldest timestamp (key) present in the buffer.
            let consumed_energy_mini_id = self
                .consumed_energy_buffer
                .keys()
                .reduce(|x, y| if x < y { x } else { y })
                .unwrap()
                .clone();

            // Check if the hardware_usage_buffer contains the key to prevent any panic/error bellow.
            if !self.hardware_usage_buffer.contains_key(&consumed_energy_mini_id) {
                todo!("decide what to do in this case");
            }

            let consumed_energy_point = self.consumed_energy_buffer.remove(&consumed_energy_mini_id).unwrap();

            // Compute the sum of every `total_usage_usec` for the given timestamp: `consumed_energy_mini_id`.
            let tot_time_sum = match self
                .global_hardware_usage_buffer
                .get(&consumed_energy_mini_id)
                .unwrap()
                .value
            {
                WrappedMeasurementValue::F64(fx) => fx,
                WrappedMeasurementValue::U64(ux) => ux as f64,
            };

            // Then for every points in the hardware_usage_buffer at `consumed_energy_mini_id`.
            for point in self
                .hardware_usage_buffer
                .remove(&consumed_energy_mini_id)
                .unwrap()
                .iter()
            {
                // We extract the current tot_time as f64.
                let cur_tot_time_f64 = match point.value {
                    WrappedMeasurementValue::F64(fx) => fx,
                    WrappedMeasurementValue::U64(ux) => ux as f64,
                };

                // Extract the attributes of the current point to add them
                // to the new measurement point.
                let point_attributes = point
                    .attributes()
                    .map(|(key, value)| (key.to_owned(), value.clone()))
                    .collect();

                // We create the new MeasurementPoint for the energy attribution.
                let new_m = MeasurementPoint::new(
                    consumed_energy_point.timestamp,
                    metric_id.unwrap(),
                    point.resource.clone(),
                    point.consumer.clone(),
                    match consumed_energy_point.value {
                        WrappedMeasurementValue::F64(fx) => cur_tot_time_f64 / tot_time_sum * fx,
                        WrappedMeasurementValue::U64(ux) => cur_tot_time_f64 / tot_time_sum * (ux as f64),
                    },
                )
                .with_attr_vec(point_attributes);

                // And finally, the MeasurementPoint is pushed to the MeasurementBuffer.
                measurements.push(new_m.clone());
            }
        }
    }

    /// Add given measurement point to the consumed energy buffer.
    fn add_to_energy_buffer(&mut self, m: MeasurementPoint) -> Result<(), TransformError> {
        match m.resource {
            // If the metric is rapl then we sum the cpu packages' value in the buffer.
            // TODO: transform this part to be more generic.
            Resource::CpuPackage { id: _ } => {
                let id = SystemTime::from(m.timestamp).duration_since(UNIX_EPOCH)?.as_nanos();
                match self.consumed_energy_buffer.get_mut(&id) {
                    Some(point) => {
                        point.value = match (point.value.clone(), m.value.clone()) {
                            (WrappedMeasurementValue::F64(fx), WrappedMeasurementValue::F64(fy)) => {
                                WrappedMeasurementValue::F64(fx + fy)
                            }
                            (WrappedMeasurementValue::U64(ux), WrappedMeasurementValue::U64(uy)) => {
                                WrappedMeasurementValue::U64(ux + uy)
                            }
                            (_, _) => unreachable!("should not receive mixed U64 and F64 values"),
                        };
                    }
                    None => {
                        self.consumed_energy_buffer.insert(id, m.clone());
                    }
                }
                Ok(())
            }
            // Resource::Gpu { bus_id: _ } => {
            //     let id = SystemTime::from(m.timestamp).duration_since(UNIX_EPOCH)?.as_nanos();
            //     self.consumed_energy_buffer.insert(id, m.clone());
            //     Ok(())
            // }
            _ => Ok(()),
        }
    }

    /// Add given measurement point to the hardware usage buffer.
    fn add_to_hardware_buffer(&mut self, m: MeasurementPoint) -> Result<(), TransformError> {
        let m_attributes = HashMap::<&str, &AttributeValue>::from_iter(m.attributes());

        // Ugly and temporary until cpu percentage is used in the calculation.
        if let Some(uid) = m_attributes.get("uid") {
            if ["besteffort", "burstable"].contains(&uid.to_string().as_str()) {
                return Ok(());
            }
        }

        // Filter the metric that we want to keep.
        if self
            .hardware_usage_metric_filter
            .clone()
            .iter()
            .any(|(key, filter_value)| {
                if let Some(attribute_value) = m_attributes.get(key.as_str()) {
                    return attribute_value.to_string().as_str() != filter_value;
                }
                false
            })
        {
            return Ok(());
        }

        let id = SystemTime::from(m.timestamp).duration_since(UNIX_EPOCH)?.as_nanos();
        match self.hardware_usage_buffer.get_mut(&id) {
            Some(vec_points) => {
                vec_points.push(m.clone());
            }
            None => {
                // If the buffer does not have any value for the current id (timestamp)
                // then we create the vec with its first value.
                self.hardware_usage_buffer.insert(id, vec![m.clone()]);
            }
        }

        Ok(())
    }

    /// Adds given measurement point to the global hardware usage buffer.
    fn add_to_global_hardware_buffer(&mut self, m: MeasurementPoint) -> Result<(), TransformError> {
        let id = SystemTime::from(m.timestamp).duration_since(UNIX_EPOCH)?.as_nanos();
        self.global_hardware_usage_buffer.insert(id, m.clone());
        Ok(())
    }

    fn get_metric_id(&self, name: String) -> Result<RawMetricId, TransformError> {
        Ok(self.metric_reader.lock().unwrap().as_ref().ok_or(anyhow!("cannot get ref of metric registry"))?.by_name(&name).ok_or(anyhow!("cannot retrieve metric by name {name}"))?.0)
    }
}

impl Transform for EnergyAttributionTransform {
    /// Applies the transform on the measurements.
    fn apply(&mut self, measurements: &mut MeasurementBuffer, _ctx: &TransformContext) -> Result<(), TransformError> {
        // Retrieve the hardware_usage_id and the consumed_energy_id.
        // let metrics = &self.metrics;

        // let hardware_usage_id = metrics.hardware_usage.unwrap().as_u64();
        // let consumed_energy_id = metrics.consumed_energy.unwrap().as_u64();
        // let global_hardware_usage_id = metrics.global_hardware_usage.unwrap().as_u64();

        // Filling the buffers.
        for m in measurements.clone().iter() {
            if m.metric.as_u64() == self.metrics.consumed_energy_id.unwrap_or(self.get_metric_id(self.metrics.consumed_energy_metric_name.clone())?).as_u64() {
                let _ = &self.add_to_energy_buffer(m.clone())?;
            } else if m.metric.as_u64() == self.metrics.hardware_usage_id.unwrap_or(self.get_metric_id(self.metrics.hardware_usage_metric_name.clone())?).as_u64() {
                let _ = &self.add_to_hardware_buffer(m.clone())?;
            } else if m.metric.as_u64() == self.metrics.global_hardware_usage_id.unwrap_or(self.get_metric_id(self.metrics.global_hardware_metric_name.clone())?).as_u64() {
                let _ = &self.add_to_global_hardware_buffer(m.clone())?;
            }
        }

        // Emptying the buffers and pushing the energy attribution to the MeasurementBuffer
        self.buffer_bouncer(measurements);

        Ok(())
    }
}
