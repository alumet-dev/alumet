use std::net::SocketAddr;

use alumet::{measurement::WrappedMeasurementValue, metrics::MetricId, pipeline::Output};

use anyhow::Context;
use http_body_util::{combinators::BoxBody, BodyExt, Full};
use hyper::{
    body::{Bytes, Incoming},
    server::conn::http1,
    service::service_fn,
    Method, Request, Response,
};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;

pub struct PrometheusOutput {
    // TODO store something that allows to communicate with the http server
}

impl Output for PrometheusOutput {
    fn write(
        &mut self,
        measurements: &alumet::measurement::MeasurementBuffer,
        ctx: &alumet::pipeline::OutputContext,
    ) -> Result<(), alumet::pipeline::WriteError> {
        // Example output (to replace by the computation of Prometheus metrics that will be exposed through the HTTP server running in parallel)

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

/// Called when an HTTP request is received (normally from Prometheus).
async fn handle_request(req: Request<Incoming>) -> Result<Response<BoxBody<Bytes, hyper::Error>>, hyper::Error> {
    /// Utility function to create a response body.
    /// See https://hyper.rs/guides/1/server/echo
    fn full<T: Into<Bytes>>(chunk: T) -> BoxBody<Bytes, hyper::Error> {
        Full::new(chunk.into()).map_err(|never| match never {}).boxed()
    }

    match (req.method(), req.uri().path()) {
        (&Method::GET, "/") => {
            Ok(Response::new(full("hello")))
        },
        (&Method::GET, "/metrics") => {
            todo!("respond to prometheus")
        },
        _ => {
            todo!("error")
        }
    }
}

/// Main HTTP server loop.
pub async fn run_http_server(addr: SocketAddr) -> anyhow::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    loop {
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);

        tokio::task::spawn(async move {
            http1::Builder::new()
                .serve_connection(io, service_fn(handle_request))
                .await
                .context("error serving HTTP connection")
        });

        // TODO: proper server shutdown
    }
    Ok(())
}
