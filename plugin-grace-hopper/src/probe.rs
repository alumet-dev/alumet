use std::{
    fs::File,
    io::{Read, Seek},
};

use anyhow::anyhow;

use alumet::{
    measurement::MeasurementPoint,
    metrics::TypedMetricId,
    pipeline::Source,
    plugin::AlumetPluginStart,
    resources::{Resource, ResourceConsumer},
    units::{PrefixedUnit, Unit},
};

use crate::SensorInformation;

pub struct GraceHopperProbe {
    socket: String,
    kind: String,
    file: File,
    consumer: ResourceConsumer,
    metric: Option<TypedMetricId<u64>>,
    _power_stats_interval: String,
}

impl GraceHopperProbe {
    pub fn new(alumet: &mut AlumetPluginStart, sensor_info: SensorInformation) -> Result<Self, anyhow::Error> {
        let metric = alumet
            .create_metric::<u64>(
                "consumption",
                PrefixedUnit::micro(Unit::Watt),
                "Power consumption of the sensor",
            )
            .ok();

        if !sensor_info.file.exists() {
            return Err(anyhow!(
                "Can't find the file: {:?} so no probe created",
                sensor_info.file
            ));
        };
        let file = File::open(
            sensor_info
                .file
                .parent()
                .expect("power1_average file should exist")
                .join("power1_average"),
        )?;
        let probe: GraceHopperProbe = GraceHopperProbe {
            socket: sensor_info.sensor.socket,
            kind: sensor_info.sensor.kind.to_lowercase(),
            file,
            metric,
            consumer: ResourceConsumer::LocalMachine,
            _power_stats_interval: sensor_info.average_interval,
        };
        Ok(probe)
    }
}

impl Source for GraceHopperProbe {
    fn poll(
        &mut self,
        measurements: &mut alumet::measurement::MeasurementAccumulator,
        timestamp: alumet::measurement::Timestamp,
    ) -> Result<(), alumet::pipeline::elements::error::PollError> {
        let power = read_power_value(&mut self.file);
        measurements.push(
            MeasurementPoint::new(
                timestamp,
                self.metric.unwrap(),
                Resource::LocalMachine,
                self.consumer.clone(),
                power.unwrap(),
            )
            .with_attr("sensor", self.kind.clone())
            .with_attr("socket", self.socket.clone()),
        );

        Ok(())
    }
}

pub fn read_power_value(file: &mut File) -> Result<u64, anyhow::Error> {
    let mut buffer = String::new();
    buffer.clear();
    file.rewind()?;
    file.read_to_string(&mut buffer)?;

    let power_consumption = match buffer.trim().parse::<u64>() {
        Ok(value) => value,
        Err(_) => {
            log::error!("Can't parse the content of file {:?}, read: {:?}", file, buffer);
            0
        }
    };
    Ok(power_consumption)
}

#[cfg(test)]
mod tests {
    use anyhow::Context;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    use crate::probe::read_power_value;

    #[test]
    fn test_read_power_value() {
        let test_cases = vec![
            ("123456789", 123456789),
            ("585865", 585865),
            ("987123", 987123),
            ("5588", 5588),
            ("0", 0),
        ];

        for (line, expected_sensor) in test_cases {
            let root = tempdir().unwrap();
            let file_path = root.path().join("power1_oem");
            let mut file = File::create(&file_path).unwrap();
            writeln!(file, "{}", line).unwrap();
            let mut file = File::open(&file_path)
                .context("Failed to open the file")
                .expect("Can't open the file when testing read_power_value function");
            let result = read_power_value(&mut file);
            assert!(result.is_ok(), "Expected Ok for input '{}'", line);
            let power = result.unwrap();
            // Check content
            assert_eq!(power, expected_sensor, "Incorrect sensor for input '{}'", line);
        }
    }
}
