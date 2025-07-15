use super::*;
use crate::kwollect::parse_measurements;
use crate::{Config, kwollect::MeasureKwollect};
use alumet::{
    measurement::{AttributeValue, MeasurementAccumulator, MeasurementPoint, Timestamp, WrappedMeasurementValue},
    metrics::{TypedMetricId, registry::MetricRegistry},
    pipeline::elements::{error::PollError, source::Source},
    resources::{Resource, ResourceConsumer},
};
use chrono::FixedOffset;
use log;
use std::time::{Duration, SystemTime};
use time::OffsetDateTime;

pub struct KwollectSource {
    pub config: Config,
    pub metric_registry: MetricRegistry,
}

impl KwollectSource {
    pub fn new(config: Config, metric_registry: MetricRegistry) -> anyhow::Result<KwollectSource> {
        Ok(KwollectSource {
            config,
            metric_registry,
        })
    }
}

impl Source for KwollectSource {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator<'_>, timestamp: Timestamp) -> Result<(), PollError> {
        log::info!("Kwollect-input plugin is starting");

        fn create_measurement_point(
            measure: &MeasureKwollect,
            metric_registry: &MetricRegistry,
            timestamp: Timestamp,
        ) -> Result<MeasurementPoint, PollError> {
            let resource = Resource::LocalMachine;
            let consumer = ResourceConsumer::LocalMachine;

            let metric_id = create_f64_metric_id(measure, metric_registry)
                .map_err(|e| PollError::Fatal(anyhow::anyhow!("{}", e)))?;

            let value = match measure.value {
                WrappedMeasurementValue::F64(v) => v,
                WrappedMeasurementValue::U64(v) => v as f64,
            };

            let measurement_point = MeasurementPoint::new(timestamp, metric_id, resource, consumer, value)
                .with_attr("device_id", AttributeValue::String(measure.device_id.clone()))
                .with_attr("metric_id", AttributeValue::String(measure.metric_id.clone()));

            Ok(measurement_point)
        }

        fn create_f64_metric_id(
            measure: &MeasureKwollect,
            metric_registry: &MetricRegistry,
        ) -> Result<TypedMetricId<f64>, anyhow::Error> {
            TypedMetricId::<f64>::try_from(
                alumet::metrics::def::RawMetricId::from_u64(measure.metric_id.parse::<u64>().unwrap_or_default()),
                metric_registry,
            )
            .map_err(|e| anyhow::Error::new(e))
        }

        let start_alumet: OffsetDateTime = SystemTime::now().into();
        let system_time: SystemTime = convert_to_system_time(start_alumet);
        let start_utc = convert_to_utc(system_time);

        std::thread::sleep(Duration::from_secs(10));

        let end_alumet: OffsetDateTime = SystemTime::now().into();
        let system_time: SystemTime = convert_to_system_time(end_alumet);
        let end_utc = convert_to_utc(system_time);

        let paris_offset = FixedOffset::east_opt(2 * 3600).unwrap();
        let start_paris = start_utc.with_timezone(&paris_offset);
        let end_paris = end_utc.with_timezone(&paris_offset);

        let url = build_kwollect_url(&self.config, &start_paris, &end_paris);

        match fetch_data(&url, &self.config) {
            Ok(data) => {
                log::info!("Raw API data: {:?}", data);
                if let Some(parsed) = parse_measurements(data) {
                    for measure in parsed {
                        match create_measurement_point(&measure, &self.metric_registry, timestamp) {
                            Ok(measurement_point) => measurements.push(measurement_point),
                            Err(e) => return Err(e),
                        }
                    }
                }
                Ok(())
            }
            Err(e) => {
                log::error!("Failed to fetch data: {}", e);
                Err(PollError::Fatal(anyhow::anyhow!("{}", e)))
            }
        }
    }
}
