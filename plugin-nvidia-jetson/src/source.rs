use std::{
    fs::File,
    io::{Read, Seek},
};

use alumet::{
    measurement::{AttributeValue, MeasurementAccumulator, MeasurementPoint, Timestamp},
    metrics::TypedMetricId,
    pipeline::elements::error::PollError,
    plugin::AlumetPluginStart,
    resources::{Resource, ResourceConsumer},
};
use anyhow::{anyhow, Context};

use super::ina::InaSensor;

/// Measurement source that queries the embedded INA3221 sensor of a Jetson device.
pub struct JetsonInaSource {
    opened_sensors: Vec<OpenedInaSensor>,
}

/// A sensor that has been "opened" for reading.
pub struct OpenedInaSensor {
    i2c_address: u32,
    device_number: u32,
    channels: Vec<OpenedInaChannel>,
}

/// A channel that has been "opened" for reading.
pub struct OpenedInaChannel {
    label: String,
    description: String,
    metrics: Vec<OpenedInaMetric>,
}

/// A channel metric that has been "opened" for reading.
pub struct OpenedInaMetric {
    /// Id of the metric registered in Alumet.
    /// The INA sensors provides integer values.
    metric_id: TypedMetricId<u64>,
    /// Id of the "resource" corresponding to the INA sensor.
    resource_id: Resource,
    /// The virtual file in the sysfs, opened for reading.
    file: File,
}

impl JetsonInaSource {
    pub fn open_sensors(sensors: Vec<InaSensor>, alumet: &mut AlumetPluginStart) -> anyhow::Result<JetsonInaSource> {
        if sensors.is_empty() {
            return Err(anyhow!("Cannot construct a JetsonInaSource without any sensor."));
        }

        let mut opened_sensors = Vec::with_capacity(4);
        for sensor in sensors {
            let mut sensor_opened_channels = Vec::with_capacity(sensor.channels.len());
            for channel in sensor.channels {
                let channel_string_id = channel.label.clone().map_or_else(
                    || format!("channel_{}", channel.id),
                    |v| v.replace(' ', "_").to_ascii_lowercase(),
                );
                let metrics: anyhow::Result<Vec<OpenedInaMetric>> = channel
                    .metrics
                    .into_iter()
                    .map(|m| {
                        let metric_description = match &channel.description {
                            Some(desc) => format!("channel {} ({}); {}", channel.id, desc, m.name),
                            None => format!("channel {}; {}", channel.id, m.name),
                        };
                        let metric_id = alumet
                            .create_metric(format!("{}::{}", channel_string_id, m.name), m.unit, metric_description)
                            .with_context(|| format!("could not create metric for channel {channel_string_id}"))?;
                        let file = File::open(&m.path)
                            .with_context(|| format!("could not open virtual file {}", m.path.display()))?;
                        Ok(OpenedInaMetric {
                            metric_id,
                            resource_id: Resource::LocalMachine,
                            file,
                        })
                    })
                    .collect();
                let opened_chan = OpenedInaChannel {
                    label: channel_string_id,
                    description: channel.description.unwrap_or(String::from("")),
                    metrics: metrics?,
                };
                sensor_opened_channels.push(opened_chan);
            }
            opened_sensors.push(OpenedInaSensor {
                i2c_address: sensor.metadata.i2c_address,
                device_number: sensor.metadata.number,
                channels: sensor_opened_channels,
            })
        }
        Ok(JetsonInaSource { opened_sensors })
    }
}

impl alumet::pipeline::Source for JetsonInaSource {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        let mut reading_buf = Vec::with_capacity(8);

        for sensor in &mut self.opened_sensors {
            for chan in &mut sensor.channels {
                for m in &mut chan.metrics {
                    // read the file from the beginning
                    reading_buf.clear();
                    m.file.rewind()?;
                    m.file.read_to_end(&mut reading_buf)?;

                    // parse the content of the file
                    let content = std::str::from_utf8(&reading_buf)?;
                    let value: u64 = content
                        .trim_end()
                        .parse()
                        .with_context(|| format!("failed to parse {:?}: '{content}", m.file))?;

                    // produce a measurement point
                    let consumer = ResourceConsumer::LocalMachine;
                    measurements.push(
                        MeasurementPoint::new(timestamp, m.metric_id, m.resource_id.clone(), consumer, value)
                            .with_attr("ina_device_number", AttributeValue::U64(sensor.device_number.into()))
                            .with_attr("ina_i2c_address", AttributeValue::U64(sensor.i2c_address.into()))
                            .with_attr("ina_channel_label", AttributeValue::String(chan.label.clone()))
                            .with_attr(
                                "ina_channel_description",
                                AttributeValue::String(chan.description.clone()),
                            ),
                    );
                }
            }
        }
        Ok(())
    }
}
