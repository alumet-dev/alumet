use alumet::{measurement::WrappedMeasurementValue, metrics::MetricId, pipeline::Output, plugin::rust::AlumetPlugin};

pub struct PrometheusPlugin {}

impl AlumetPlugin for PrometheusPlugin {
    fn name() -> &'static str {
        "prometheus-exporter"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn init(_config: alumet::plugin::ConfigTable) -> anyhow::Result<Box<Self>> {
        Ok(Box::new(PrometheusPlugin {}))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetStart) -> anyhow::Result<()> {
        alumet.add_output(Box::new(PrometheusOutput {}));
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

struct PrometheusOutput {}

impl Output for PrometheusOutput {
    fn write(
        &mut self,
        measurements: &alumet::measurement::MeasurementBuffer,
        ctx: &alumet::pipeline::OutputContext,
    ) -> Result<(), alumet::pipeline::WriteError> {
        // Example output (to replace by the computation of Prometheus metrics that will be exposed through an HTTP server running in the background)
        
        for m in measurements {
            let metric_id = m.metric;
            let metric_name = metric_id.name(ctx);
            // let full_metric = ctx.metrics.with_id(&metric_id).unwrap();
            let value = &m.value;
            let timestamp = m.timestamp;
            let value_str = match &value {
                WrappedMeasurementValue::F64(float) => float.to_string(),
                WrappedMeasurementValue::U64(integer) => integer.to_string(),
            };
            let attributes_str = m
                .attributes()
                .map(|(key, value)| format!("{key}={value}"))
                .collect::<Vec<_>>()
                .join(" ");
            println!("{timestamp:?} {metric_name}={value_str}; {attributes_str}");
        }
        Ok(())
    }
}
