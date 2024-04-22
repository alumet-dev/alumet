use alumet::plugin::util::CounterDiffUpdate;
use alumet::measurement::MeasurementAccumulator;
use std::fs::File;
use alumet::measurement::AttributeValue;
use alumet::measurement::MeasurementPoint;
use alumet::{
    measurement::Timestamp,
    metrics::TypedMetricId,
    metrics::MetricCreationError,
    plugin::util::CounterDiff,
    plugin::AlumetStart,
    resources::ResourceId,
    units::Unit,
};
use anyhow::{Context, Result};
use std::time::SystemTime;
use std::time::UNIX_EPOCH;


use crate::parsing_cgroupv2::{self, CgroupV2Metric};

pub(crate) const CGROUP_MAX_TS: u64 = u64::MAX;


/// Energy probe based on perf_event for intel RAPL.
pub struct K8SProbe {
    pub name: String,
    pub metrics: Metrics,
}

#[derive(Clone)]
pub struct Metrics {
    pub time_used_tot: TypedMetricId<u64>,
    pub time_used_user_mode: TypedMetricId<u64>,
    pub time_used_system_mode: TypedMetricId<u64>,
}

impl K8SProbe {
    pub fn new(metric: Metrics,name: String) -> anyhow::Result<K8SProbe> {
        println!("New implem of K8S prob called");
        
        // let mut opened = Vec::with_capacity(pods.len());
        // for (event, CpuId { cpu, socket }) in pods {
        //     let raw_fd = event
        //         .perf_event_open(pmu_type, *cpu)
        //         .with_context(|| format!("perf_event_open failed. {ADVICE}"))?;
        //     let fd = unsafe { File::from_raw_fd(raw_fd) };
        //     let scale = event.scale as f64;
        //     let opened_event = OpenedPowerEvent {
        //         fd,
        //         scale,
        //         socket: *socket,
        //         domain: event.domain,
        //         resource: event.domain.to_resource(*socket),
        //     };
        //     let counter = CounterDiff::with_max_value(PERF_MAX_ENERGY);
        //     opened.push((opened_event, pods))
        // }
        Ok(K8SProbe{
        name: name,
        metrics: metric,
        })
    }
}

impl alumet::pipeline::Source for K8SProbe {
    fn poll(
        &mut self,
        measurements: &mut MeasurementAccumulator,
        timestamp: Timestamp,
    ) -> Result<(), alumet::pipeline::PollError> {
        // for (evt, counter) in &mut self.events {
        //     // read the new value of the perf-events counter
        //     let counter_value = read_perf_event(&mut evt.fd)
        //         .with_context(|| format!("failed to read perf_event {:?} for domain {:?}", evt.fd, evt.domain))?;

        //     // correct any overflows
        //     let diff = match counter.update(counter_value) {
        //         alumet::util::CounterDiffUpdate::FirstTime => None,
        //         alumet::util::CounterDiffUpdate::Difference(diff) => Some(diff),
        //         alumet::util::CounterDiffUpdate::CorrectedDifference(diff) => {
        //             log::debug!("Overflow on perf_event counter for RAPL domain {}", evt.domain);
        //             Some(diff)
        //         }
        //     };
        //     if let Some(value) = diff {
        //         // convert to joules and push
        //         let joules = (value as f64) * evt.scale;
        //         measurements.push(MeasurementPoint::new(
        //             timestamp,
        //             self.metric,
        //             evt.resource.clone(),
        //             joules,
        //         ).with_attr("domain", AttributeValue::String(evt.domain.to_string())));
        //     }
        //     // NOTE: the energy can be a floating-point number in Joules,
        //     // without any loss of precision. Why? Because multiplying any number
        //     // by a float that is a power of two will only change the "exponent" part,
        //     // not the "mantissa", and the energy unit for RAPL is always a power of two.
        //     //
        //     // A f32 can hold integers without any precision loss
        //     // up to approximately 2^24, which is not enough for the RAPL counter values,
        //     // so we use a f64 here.
        // }
        // for metric in 

        let since_the_epoch = SystemTime::now().duration_since(UNIX_EPOCH).expect("Time went backwards");
        println!("Poll Called at {:?}", since_the_epoch); 
        // let mut rng = rand::thread_rng();
        // let final_value: f32 = (rng.gen::<f32>())*100.0;        
        // let p1: MeasurementPoint = MeasurementPoint::new(timestamp, self.energy_a, self.resource.clone(), final_value.into());
        // let p2: MeasurementPoint = MeasurementPoint::new(timestamp, self.temperature_a, self.resource.clone(), final_value.into());
        // measurements.push(p1);
        // measurements.push(p2);

        Ok(())
    }
}


impl Metrics {
    pub fn new(alumet: &mut AlumetStart) -> Result<Self, MetricCreationError> {
        let usec = Unit::Custom {
            unique_name: "usec".to_owned(),
            display_name: "µsec".to_owned(),
        };
        Ok(Self {
        time_used_tot: alumet.create_metric::<u64>("total_usage_usec", usec.clone(), "Total CPU usage time by the group")?, 
        time_used_user_mode: alumet.create_metric::<u64>("user_usage_usec", usec.clone(), "User CPU usage time by the group")?,
        time_used_system_mode: alumet.create_metric::<u64>("system_usage_usec", usec.clone(), "System CPU usage time by the group")?,
        })
    }
}
