use std::collections::HashMap;

use alumet::{
    measurement::{AttributeValue, MeasurementBuffer},
    pipeline::elements::{error::WriteError, output::OutputContext},
};
use anyhow::Context;
use reqwest::{blocking::Client, StatusCode};

use crate::kwollect::Measure;

pub struct KwollectOutput {
    client: Client,
    url: String,
    node: String,
    auth: Option<(String, String)>,
}

impl KwollectOutput {
    pub fn new(url: String, node: String, login: Option<String>, password: Option<String>) -> anyhow::Result<Self> {
        if let (Some(user), Some(pass)) = (login, password) {
            Ok(Self {
                client: Client::builder().danger_accept_invalid_certs(true).build()?,
                url,
                node,
                auth: Some((user, pass)),
            })
        } else {
            Ok(Self {
                client: Client::builder().danger_accept_invalid_certs(true).build()?,
                url,
                node,
                auth: None,
            })
        }
    }
}

impl alumet::pipeline::Output for KwollectOutput {
    fn write(&mut self, measurements: &MeasurementBuffer, ctx: &OutputContext) -> Result<(), WriteError> {
        // let mut json_list = Value::Array(vec![]);
        let mut json_list = Vec::new();
        for measure in measurements.iter() {
            log::info!("MEASURE");
            let full_metric = ctx
                .metrics
                .by_id(&measure.metric)
                .with_context(|| format!("Unknown metric {:?}", measure.metric))?;
            let metric_name = full_metric.name.clone();
            let ts_tmp = measure.timestamp.to_unix_timestamp();
            let ts = ts_tmp.0 as f64 + ts_tmp.1 as f64 / 1_000_000_000.0;
            let mut json_map: HashMap<String, AttributeValue> = HashMap::new();
            let attrs = measure.attributes();
            for att in attrs {
                json_map.insert(att.0.to_string(), att.1.clone());
            }
            let entry = Measure {
                timestamp: ts,
                metric_id: metric_name,
                value: measure.value.clone(),
                device_id: self.node.clone(),
                labels: json_map,
            };
            let serialized = serde_json::to_value(&entry).unwrap();
            json_list.push(serialized);
        }

        let mut request_builder = self.client.post(&self.url);
        if let Some((user, pass)) = &self.auth {
            request_builder = request_builder.basic_auth(user, Some(pass));
        }
        let res = request_builder.json(&json_list).send().unwrap();

        if res.status() != StatusCode::OK {
            let body = res.text().unwrap();
            log::error!("response from remote: {}", body)
        }

        Ok(())
    }
}
