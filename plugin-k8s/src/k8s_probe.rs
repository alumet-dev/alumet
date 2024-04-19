use alumet::{
    metrics::{AttributeValue, MeasurementPoint, TypedMetricId},
    util::CounterDiff,
    resources::ResourceId,
    units::Unit,
};

use crate::parsing_cgroupv2::{self, CgroupV2Metric};

pub(crate) const PERF_MAX_ENERGY: u64 = u64::MAX;


/// Energy probe based on perf_event for intel RAPL.
pub struct K8SProbe {
    /// Id of the metric to push.
    metric: TypedMetricId<f64>,
    /// Ready-to-use power events with additional metadata.
    pods: Vec<parsing_cgroupv2::CgroupV2Metric>,
}

struct OpenedPowerEvent {
    fd: File,
    scale: f64,
    socket: u32,
    domain: RaplDomainType,
    resource: ResourceId,
}

impl K8SProbe {
    pub fn new(metric: TypedMetricId<f64>, pods: Vec<parsing_cgroupv2::CgroupV2Metric>) -> anyhow::Result<K8SProbe> {

        let mut opened = Vec::with_capacity(pods.len());
        for (event, CpuId { cpu, socket }) in pods {
            let raw_fd = event
                .perf_event_open(pmu_type, *cpu)
                .with_context(|| format!("perf_event_open failed. {ADVICE}"))?;
            let fd = unsafe { File::from_raw_fd(raw_fd) };
            let scale = event.scale as f64;
            let opened_event = OpenedPowerEvent {
                fd,
                scale,
                socket: *socket,
                domain: event.domain,
                resource: event.domain.to_resource(*socket),
            };
            let counter = CounterDiff::with_max_value(PERF_MAX_ENERGY);
            opened.push((opened_event, pods))
        }
        Ok(K8SProbe { metric, pods })
    }
}

impl alumet::pipeline::Source for K8SProbe {
    fn poll(
        &mut self,
        measurements: &mut alumet::metrics::MeasurementAccumulator,
        timestamp: std::time::SystemTime,
    ) -> Result<(), alumet::pipeline::PollError> {
        for (evt, counter) in &mut self.events {
            // read the new value of the perf-events counter
            let counter_value = read_perf_event(&mut evt.fd)
                .with_context(|| format!("failed to read perf_event {:?} for domain {:?}", evt.fd, evt.domain))?;

            // correct any overflows
            let diff = match counter.update(counter_value) {
                alumet::util::CounterDiffUpdate::FirstTime => None,
                alumet::util::CounterDiffUpdate::Difference(diff) => Some(diff),
                alumet::util::CounterDiffUpdate::CorrectedDifference(diff) => {
                    log::debug!("Overflow on perf_event counter for RAPL domain {}", evt.domain);
                    Some(diff)
                }
            };
            if let Some(value) = diff {
                // convert to joules and push
                let joules = (value as f64) * evt.scale;
                measurements.push(MeasurementPoint::new(
                    timestamp,
                    self.metric,
                    evt.resource.clone(),
                    joules,
                ).with_attr("domain", AttributeValue::String(evt.domain.to_string())));
            }
            // NOTE: the energy can be a floating-point number in Joules,
            // without any loss of precision. Why? Because multiplying any number
            // by a float that is a power of two will only change the "exponent" part,
            // not the "mantissa", and the energy unit for RAPL is always a power of two.
            //
            // A f32 can hold integers without any precision loss
            // up to approximately 2^24, which is not enough for the RAPL counter values,
            // so we use a f64 here.
        }
        Ok(())
    }
}
