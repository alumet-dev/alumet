//! Synchronization and conversion of metric ids between the clients and the server.

use alumet::{
    measurement::MeasurementBuffer,
    metrics::{Metric, RawMetricId},
    pipeline::registry::MetricSender,
};
use anyhow::{anyhow, Context};

const MAX_METRIC_ID: usize = 65535;

pub struct MetricIds {
    id_client_to_server: nohash_hasher::IntMap<u64, u64>,
    id_server_to_client: nohash_hasher::IntMap<u64, u64>,
}

pub struct MetricConverter {
    inner: MetricSender,
    client_tag: String,
    ids: MetricIds,
}

impl MetricConverter {
    pub fn new(tx: MetricSender, client_tag: String) -> Self {
        let ids = MetricIds {
            id_client_to_server: nohash_hasher::IntMap::with_capacity_and_hasher(64, Default::default()),
            id_server_to_client: nohash_hasher::IntMap::with_capacity_and_hasher(64, Default::default()),
        };
        Self {
            inner: tx,
            client_tag,
            ids,
        }
    }

    pub async fn register_from_client(&mut self, metric_ids: Vec<u64>, metric_defs: Vec<Metric>) -> anyhow::Result<()> {
        let results = self
            .inner
            .create_metrics(
                metric_defs,
                alumet::pipeline::registry::DuplicateStrategy::Rename {
                    suffix: self.client_tag.clone(),
                },
            )
            .await
            .map_err(|e| anyhow!("create_metrics returned an error: {e:?}"))?;

        for res in results.into_iter().zip(metric_ids) {
            match res {
                (Ok(server_metric_id), client_metric_id) => {
                    if client_metric_id as usize > MAX_METRIC_ID {
                        return Err(anyhow!("invalid client metric id: {client_metric_id} should be less than the maximum {MAX_METRIC_ID}"));
                    }
                    let server_metric_id = server_metric_id.as_u64();
                    self.ids.id_server_to_client.insert(server_metric_id, client_metric_id);
                    self.ids.id_client_to_server.insert(client_metric_id, server_metric_id);
                }
                (Err(e), client_metric_id) => {
                    log::error!(
                        "metric registration failed: client_metric_id={client_metric_id}, client_tag='{}'; {e:?}",
                        self.client_tag
                    );
                }
            }
        }
        Ok(())
    }

    pub fn convert_from_client(&self, client_metric_id: u64) -> Option<u64> {
        self.ids.id_client_to_server.get(&client_metric_id).copied()
    }

    pub fn convert_all(&self, buffer: &mut MeasurementBuffer) -> anyhow::Result<()> {
        for m in buffer.iter_mut() {
            let converted_id = self
                .convert_from_client(m.metric.as_u64())
                .context("invalid metric in measurement")?;
            m.metric = RawMetricId::from_u64(converted_id);
        }
        Ok(())
    }
}
