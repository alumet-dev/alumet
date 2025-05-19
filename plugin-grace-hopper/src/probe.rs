use std::{
    fs::File,
    io::{Read, Seek},
    path::Path,
};

use alumet::{
    measurement::MeasurementPoint,
    metrics::TypedMetricId,
    pipeline::Source,
    plugin::AlumetPluginStart,
    resources::{Resource, ResourceConsumer},
    units::{PrefixedUnit, Unit},
};

pub struct GraceHopperProbe {
    socket: String,
    kind: String,
    file: Option<File>,
    consumer: ResourceConsumer,
    metric: Option<TypedMetricId<u64>>,
}

impl GraceHopperProbe {
    pub fn new(
        alumet: &mut AlumetPluginStart,
        socket: String,
        kind: String,
        filepath: Option<&Path>,
    ) -> Result<Self, anyhow::Error> {
        let sensor_kind = kind.to_lowercase();
        let metric = alumet
            .create_metric::<u64>(
                "consumption",
                PrefixedUnit::micro(Unit::Watt),
                "Power consumption of the sensor",
            )
            .ok();
        let mut probe: GraceHopperProbe = GraceHopperProbe {
            socket,
            kind: sensor_kind,
            file: None,
            metric,
            consumer: ResourceConsumer::LocalMachine,
        };

        if let Some(filepath) = filepath {
            probe.file = Some(File::open(filepath.join("power1_average"))?);
        }
        Ok(probe)
    }
}

impl Source for GraceHopperProbe {
    fn poll(
        &mut self,
        measurements: &mut alumet::measurement::MeasurementAccumulator,
        timestamp: alumet::measurement::Timestamp,
    ) -> Result<(), alumet::pipeline::elements::error::PollError> {
        let mut buffer = String::new();
        if let Some(file) = &mut self.file {
            buffer.clear();
            file.rewind()?;
            file.read_to_string(&mut buffer)?;

            let power_consumption = match buffer.trim().parse::<u64>() {
                Ok(value) => value,
                Err(_) => {
                    log::error!("Can't parse the content of file {:?}, read: {:?}", self.file, buffer);
                    0
                }
            };
            measurements.push(
                MeasurementPoint::new(
                    timestamp,
                    self.metric.unwrap(),
                    Resource::LocalMachine,
                    self.consumer.clone(),
                    power_consumption,
                )
                .with_attr("Sensor", self.kind.clone())
                .with_attr("Socket", self.socket.clone()),
            )
        }

        Ok(())
    }
}
