use std::collections::HashMap;

use alumet::{measurement::MeasurementBuffer, pipeline::elements::output::OutputContext};
use anyhow::Context;
use reqwest::{
    Url,
    header::{HeaderMap, HeaderValue, InvalidHeaderValue},
};

use super::ser::{CreateIndexTemplate, IndexTemplate, Serializer};

// OpenSearch/ElasticSearch API client.
pub struct Client {
    serializer: Serializer,
    client: reqwest::blocking::Client,
    server_url: Url,
}

pub struct ConnectionSettings {
    pub auth: ApiAuthentication,
    pub server_url: Url,
    pub allow_insecure: bool,
}

pub struct DataSettings {
    pub index_prefix: String,
    pub metric_unit_as_index_suffix: bool,
}

pub enum ApiAuthentication {
    ApiKey { key: String },
    Basic { user: String, password: String },
    Bearer { token: String },
}

pub fn user_agent_string() -> String {
    let core_version = alumet::VERSION;
    let plugin_crate_name = env!("CARGO_PKG_NAME");
    let plugin_version = env!("CARGO_PKG_VERSION");
    let user_agent = format!("Alumet/{core_version} {plugin_crate_name}/{plugin_version}");
    log::debug!("Client user agent: {user_agent}");
    user_agent
}

impl Client {
    pub fn new(conn: ConnectionSettings, data: DataSettings) -> anyhow::Result<Self> {
        let mut builder = reqwest::blocking::ClientBuilder::new();
        if conn.allow_insecure {
            builder = builder
                .danger_accept_invalid_certs(true)
                .danger_accept_invalid_hostnames(true);
        }
        builder = builder
            .user_agent(user_agent_string())
            .default_headers(HeaderMap::from_iter(vec![(
                reqwest::header::AUTHORIZATION,
                conn.auth
                    .into_header_value()
                    .context("auth should make a valid http header")?,
            )]));
        let client = builder.build().context("failed to initialize HTTP client")?;
        Ok(Self {
            client,
            server_url: conn.server_url,
            serializer: Serializer {
                index_prefix: data.index_prefix,
                metric_unit_as_index_suffix: data.metric_unit_as_index_suffix,
            },
        })
    }

    pub fn create_index_template(&self) -> anyhow::Result<()> {
        const TEMPLATE_NAME: &str = "alumet_index_template";

        let template = IndexTemplate {
            mappings: self.serializer.common_index_mappings(),
        };
        let index_pattern = format!("{}-*", self.serializer.index_prefix);
        let create = CreateIndexTemplate {
            index_patterns: vec![index_pattern],
            template,
            priority: 80,
            version: 3,
            meta: HashMap::from_iter([("origin".to_string(), "Alumet measurements".to_string())]),
        };

        // Create the template (or update it if it exists).
        // Note: server_url is turned into a string with a trailing '/'.
        let url = format!("{}_index_template/{TEMPLATE_NAME}", self.server_url);
        let request = self.client.put(url).json(&create);
        log::trace!("sending {request:?}");

        let response = request.send().context("could not send request")?;
        log::trace!("got response {response:?}");

        Self::handle_response(response)?;
        Ok(())
    }

    pub fn bulk_insert_measurements(&self, m: &MeasurementBuffer, ctx: &OutputContext) -> anyhow::Result<()> {
        let url = self.server_url.join("_bulk").unwrap();

        let body = self
            .serializer
            .body_bulk_create_docs(m, ctx)
            .context("measurements serialization failed")?;

        log::trace!("serialized measurements:\n{body}");
        let request = self.client.put(url).body(body).header(
            reqwest::header::CONTENT_TYPE,
            HeaderValue::from_static("application/x-ndjson"),
        );
        log::trace!("sending {request:?}");
        let response = request.send().context("could not send request")?;

        log::trace!("got response {response:?}");
        Self::handle_response(response)?;
        Ok(())
    }

    fn handle_response(response: reqwest::blocking::Response) -> anyhow::Result<()> {
        let status = response.status();
        if status.is_client_error() || status.is_server_error() {
            let status_msg = format!("{} {}", status.as_str(), status.canonical_reason().unwrap_or_default());
            let body = response
                .text()
                .with_context(|| format!("failed to decode response body for error {status:?}"))?;
            Err(anyhow::anyhow!("server responded with error: {status_msg}\n{body}"))
        } else {
            Ok(())
        }
    }
}

impl ApiAuthentication {
    pub fn into_header_value(self) -> Result<HeaderValue, InvalidHeaderValue> {
        use base64::prelude::BASE64_STANDARD;
        use base64::write::EncoderWriter;
        use std::io::Write;

        let mut header = match self {
            ApiAuthentication::ApiKey { key } => {
                let header_value = format!("ApiKey {key}");
                HeaderValue::from_str(&header_value)
            }
            ApiAuthentication::Basic { user, password } => {
                // Unfortunately, reqwest::util::basic_auth is not accessible from the outside,
                // so we must implement it here.
                let mut buf = b"Basic ".to_vec();
                {
                    let mut encoder = EncoderWriter::new(&mut buf, &BASE64_STANDARD);
                    let _ = write!(encoder, "{user}:{password}");
                }
                HeaderValue::from_bytes(&buf)
            }
            ApiAuthentication::Bearer { token } => {
                let header_value = format!("Bearer {token}");
                HeaderValue::from_str(&header_value)
            }
        }?;
        header.set_sensitive(true);
        Ok(header)
    }
}
