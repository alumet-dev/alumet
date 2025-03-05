use anyhow::{anyhow, Context};
use std::fs::File;

use alumet::{plugin::AlumetPluginStart, resources::Resource};

use crate::jetson::utils::*;

impl JetsonInaSource {
    pub fn open_sensors(sensors: Vec<InaSensor>, alumet: &mut AlumetPluginStart) -> anyhow::Result<JetsonInaSource> {
        if sensors.is_empty() {
            return Err(anyhow!("Cannot construct a JetsonInaSource without any sensor."));
        }

        let mut opened_sensors = Vec::with_capacity(4);
        for sensor in sensors {
            let mut sensor_opened_channels = Vec::with_capacity(sensor.channels.len());
            for channel in sensor.channels {
                let metrics: anyhow::Result<Vec<OpenedInaMetric>> = channel
                    .metrics
                    .into_iter()
                    .map(|m| {
                        let metric_description = match &channel.description {
                            Some(desc) => format!("channel {} ({}); {}", channel.id, desc, m.name),
                            None => format!("channel {}; {}", channel.id, m.name),
                        };
                        let metric_id = alumet.create_metric(
                            format!("{}::{}", channel.label, m.name),
                            m.unit,
                            metric_description,
                        )?;
                        let file = File::open(&m.path)
                            .with_context(|| format!("Could not open virtual file {}", m.path.display()))?;
                        Ok(OpenedInaMetric {
                            metric_id,
                            resource_id: Resource::LocalMachine,
                            file,
                        })
                    })
                    .collect();
                let opened_chan = OpenedInaChannel {
                    label: channel.label,
                    description: channel.description.unwrap_or(String::from("")),
                    metrics: metrics?,
                };
                sensor_opened_channels.push(opened_chan);
            }
            opened_sensors.push(OpenedInaSensor {
                i2c_id: sensor.i2c_id,
                channels: sensor_opened_channels,
            })
        }
        Ok(JetsonInaSource { opened_sensors })
    }
}
