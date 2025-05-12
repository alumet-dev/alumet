use std::{
    collections::HashMap,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use alumet::{
    measurement::{AttributeValue, MeasurementBuffer, MeasurementPoint, Timestamp},
    pipeline::{
        elements::{error::TransformError, transform::TransformContext},
        Transform,
    },
    resources::{Resource, ResourceConsumer},
};

pub struct EnergyAttributionTransform {
    pub metrics: super::Metrics,
    buffer: HashMap<u64, Measurements>,
    nb_cores: usize,
}

#[derive(Debug, Default)]
struct Measurements {
    energy_by_resource: HashMap<Resource, Vec<MeasurementPoint>>,
    usage_by_consumer: HashMap<ResourceConsumer, Vec<MeasurementPoint>>,
}

impl EnergyAttributionTransform {
    /// Instantiates a new EnergyAttributionTransform with its private fields initialized.
    pub fn new(metrics: super::Metrics) -> Self {
        let nb_cores = if metrics.divide_usage_by_core_count { num_cpus::get() } else { 1 };
        Self {
            metrics,
            buffer: HashMap::new(),
            nb_cores,
        }
    }
}

impl Transform for EnergyAttributionTransform {
    /// Applies the transform on the measurements.
    fn apply(&mut self, measurements: &mut MeasurementBuffer, _ctx: &TransformContext) -> Result<(), TransformError> {
        let energy_metric = self.metrics.consumed_energy;
        let usage_metric = self.metrics.hardware_usage;

        fn pass_attr_filter(m: &MeasurementPoint, filter: &Option<(String, String)>) -> bool {
            match filter {
                Some((key, value)) => m
                    .attributes()
                    .find(|(k, v)| {
                        k == key
                            && (matches!(v, AttributeValue::String(v) if v == value)
                                || matches!(v, AttributeValue::Str(s) if s == value))
                    })
                    .is_some(),
                None => true,
            }
        }

        // fill buffers
        for m in measurements.iter() {
            let t_sec = m.timestamp.to_unix_timestamp().0; // take the whole second only => does not work if freq > 1Hz
            if m.metric == energy_metric && pass_attr_filter(m, &self.metrics.filter_energy_attr) {
                self.buffer
                    .entry(t_sec)
                    .or_insert_with(Default::default)
                    .energy_by_resource
                    .entry(m.resource.clone())
                    .or_insert_with(Default::default)
                    .push(m.clone());
            } else if m.metric == usage_metric {
                self.buffer
                    .entry(t_sec)
                    .or_insert_with(Default::default)
                    .usage_by_consumer
                    .entry(m.consumer.clone())
                    .or_insert_with(Default::default)
                    .push(m.clone());
            }
        }

        // compute energy attribution
        self.buffer.retain(|t_sec, data| {
            if data.energy_by_resource.is_empty() || data.usage_by_consumer.is_empty() {
                true // keep this buffer a bit longer (wait for more data)
            } else {
                // LIMITATION: only works at freq >= 1Hz and if hardware_usage_poll_interval matches the pol_interval of the hardware usage
                let dt = self.metrics.hardware_usage_poll_interval.as_secs_f64();

                let timestamp = Timestamp::from(UNIX_EPOCH.checked_add(Duration::from_secs(*t_sec)).unwrap());

                // for each hardware resource (for example, for each CPU package)
                for (resource, buf) in &data.energy_by_resource {
                    // the energy consumed by the hardware, in Joules
                    let total_energy: f64 = buf.iter().map(|m| m.value.as_f64()).sum();

                    // attribute to each consumer
                    for (consumer, buf) in std::mem::take(&mut data.usage_by_consumer) {
                        let consumer_usage: f64 = buf.iter().map(|m| m.value.as_f64()).sum();

                        // get consumption in f64 seconds
                        let factor = self.metrics.hardware_usage_unit.prefix.scale_f64();
                        let consumer_usage_sec = consumer_usage * factor;

                        // compute fraction and attribute energy
                        log::trace!("attributed_energy({consumer:?}) = {consumer_usage_sec} s / {dt} s / {}", self.nb_cores);
                        let consumer_usage_fraction = consumer_usage_sec / dt / (self.nb_cores as f64);
                        let attributed_energy = total_energy * consumer_usage_fraction;

                        // push the result
                        measurements.push(MeasurementPoint::new(
                            timestamp,
                            self.metrics.attributed_energy,
                            resource.clone(),
                            consumer.clone(),
                            attributed_energy,
                        ));
                    }
                }
                // remove this buffer if it's old enough, otherwise keep it a little bit in case more hardware usage arrives
                let remove = SystemTime::now()
                    .duration_since(UNIX_EPOCH.checked_add(Duration::from_secs(*t_sec)).unwrap())
                    .map(|d| d.as_secs_f64() >= 2.0)
                    .unwrap_or(false);
                !remove
            }
        });

        Ok(())
    }
}
