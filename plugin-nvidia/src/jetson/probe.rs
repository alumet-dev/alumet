use anyhow::{anyhow, Context};
use std::io::{Read, Seek};

use alumet::{
    measurement::{AttributeValue, MeasurementAccumulator, MeasurementPoint, Timestamp},
    pipeline::elements::error::PollError,
    resources::ResourceConsumer,
};

use crate::jetson::utils::JetsonInaSource;

impl alumet::pipeline::Source for JetsonInaSource {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        let mut reading_buf = Vec::with_capacity(8);

        for sensor in &mut self.opened_sensors {
            for chan in &mut sensor.channels {
                for m in &mut chan.metrics {
                    // read the file from the beginning
                    m.file.rewind()?;
                    m.file.read_to_end(&mut reading_buf)?;

                    // parse the content of the file
                    let content = std::str::from_utf8(&reading_buf)?;
                    let value = content
                        .trim_end()
                        .parse()
                        .with_context(|| format!("failed to parse {:?}: '{content}", m.file))?;

                    // store the value and clear the buffer
                    let consumer = ResourceConsumer::LocalMachine;
                    measurements.push(
                        MeasurementPoint::new(timestamp, m.metric_id, m.resource_id.clone(), consumer, value)
                            .with_attr("jetson_ina_sensor", AttributeValue::String(sensor.i2c_id.clone()))
                            .with_attr("jetson_ina_channel_label", AttributeValue::String(chan.label.clone()))
                            .with_attr(
                                "jetson_ina_channel_description",
                                AttributeValue::String(chan.description.clone()),
                            ),
                    );
                    reading_buf.clear();
                }
            }
        }
        Ok(())
    }
}
