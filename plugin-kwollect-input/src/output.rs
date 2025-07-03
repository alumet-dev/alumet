use std::collections::HashMap;

use alumet::{
    measurement::{AttributeValue, MeasurementBuffer},
    metrics,
    pipeline::elements::{error::WriteError, output::OutputContext},
};
use anyhow::Context;

pub struct KwollectInput {
    url: String,
    site: String,
    hostname: String,
    metrics: String,
}

impl KwollectInput {
    pub fn new(url: String, site: String, hostname: String, metrics: String) -> anyhow::Result<Self> {
        Ok(Self {
            url,
            site,
            hostname,
            metrics,
        })
    }
}

impl alumet::pipeline::Output for KwollectInput {
    fn write(&mut self, measurements: &MeasurementBuffer, ctx: &OutputContext) -> Result<(), WriteError> {
        todo!() // use csv plugin here???
    }
}
