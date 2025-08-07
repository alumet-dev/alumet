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
    id: u32,
    label: String,
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
                let channel_label = channel
                    .label
                    .clone()
                    .map_or_else(|| channel.id.to_string(), |v| v.replace(' ', "_").to_ascii_uppercase());
                let metrics: anyhow::Result<Vec<OpenedInaMetric>> = channel
                    .metrics
                    .into_iter()
                    .map(|m| {
                        // Open the file for the measurement operation.
                        let file = File::open(&m.path)
                            .with_context(|| format!("could not open virtual file {}", m.path.display()))?;

                        // Create a metric in Alumet. Duplicates are ok (Alumet will only create each metric once).
                        let metric_id = alumet
                            .create_metric(&m.name, m.unit, format!("{} (see attributes for channel info)", m.name))
                            .with_context(|| format!("could not create metric for channel {channel_label}"))?;

                        Ok(OpenedInaMetric {
                            metric_id,
                            resource_id: Resource::LocalMachine,
                            file,
                        })
                    })
                    .collect();
                let opened_chan = OpenedInaChannel {
                    id: channel.id,
                    label: channel_label,
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
                            .with_attr("ina_channel_id", AttributeValue::U64(chan.id.into()))
                            .with_attr("ina_channel_label", AttributeValue::String(chan.label.clone())),
                    );
                }
            }
        }
        Ok(())
    }
}
