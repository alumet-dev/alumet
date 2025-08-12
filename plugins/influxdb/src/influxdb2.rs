//! InfluxDB2 API.

use alumet::measurement::Timestamp;
use anyhow::Context;
use reqwest::{header, Url};
use std::{
    borrow::Cow,
    fmt::Write,
    time::{SystemTime, UNIX_EPOCH},
};

/// Client for InfluxDB v2.
pub struct Client {
    client: reqwest::Client,
    /// String of the form `<host>/api/v2/write`.
    write_url: String,
    /// String of the form `Token <api_token>`.
    token_header: String,
}

impl Client {
    pub fn new(host: String, token: String) -> Self {
        let write_url = format!("{host}/api/v2/write");
        let token = format!("Token {token}");
        Self {
            client: reqwest::Client::new(),
            write_url,
            token_header: token,
        }
    }

    /// Writes measurements to InfluxDB, in the given organization and bucket.
    pub async fn write(&self, org: &str, bucket: &str, data: LineProtocolData) -> anyhow::Result<()> {
        // TODO optimize: https://docs.influxdata.com/influxdb/v2/write-data/best-practices/optimize-writes
        let precision = "ns";
        let url = Url::parse_with_params(
            &self.write_url,
            &[("org", org), ("bucket", bucket), ("precision", precision)],
        )?;
        let res = self
            .client
            .post(url)
            .header(header::AUTHORIZATION, &self.token_header)
            .header(header::ACCEPT, "application/json")
            .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
            .body(data.0)
            .send()
            .await?;
        match res.error_for_status_ref() {
            Ok(_) => (),
            Err(err) => {
                let response = res.text().await.context("failed to get a response from the server")?;
                log::error!("InfluxDB2 client error: {err}\nServer response: {response}");
                return Err(err.into());
            }
        }
        Ok(())
    }

    /// Tests whether it is possible to write to the given organization and bucket with the client.
    ///
    /// Returns `Ok(())` if all goes well.
    pub async fn test_write(&self, org: &str, bucket: &str) -> anyhow::Result<()> {
        // send empty data
        self.write(org, bucket, LineProtocolData(String::new())).await
    }
}

#[derive(Debug)]
pub struct LineProtocolData(String);

impl LineProtocolData {
    pub fn builder() -> LineProtocolBuilder {
        LineProtocolBuilder::new()
    }
}

pub struct LineProtocolBuilder {
    buf: String,
    after_first_field: bool,
}

#[allow(unused)]
impl LineProtocolBuilder {
    pub fn new() -> Self {
        Self {
            buf: String::new(),
            after_first_field: false,
        }
    }

    #[allow(unused)]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buf: String::with_capacity(capacity),
            after_first_field: false,
        }
    }

    /// Writes the measurement to the current line.
    ///
    /// Must be called first in a line. Required.
    pub fn measurement(&mut self, name: &str) -> &mut Self {
        if self.after_first_field {
            self.after_first_field = false;
            self.buf.push('\n'); // new measurement
        }
        self.buf.push_str(&escape_string(name, &[',', ' ']));
        self
    }

    /// Writes a tag to the current line.
    ///
    /// Must be called after `measurement`. Optional.
    pub fn tag(&mut self, key: &str, value: &str) -> &mut Self {
        // tag values cannot be empty!
        if !value.is_empty() {
            let key = escape_string(key, &[',', '=', ' ']);
            let value = escape_string(value, &[',', '=', ' ']);
            write!(self.buf, ",{key}={value}").unwrap();
        }
        self
    }

    /// Writes a field to the current line.
    ///
    /// Must be called after `tag` (or `measurement` if there's no tag).
    /// Required (there must be at least one field).
    fn field(&mut self, key: &str, serialized_value: &str) -> &mut Self {
        let key = escape_string(key, &[',', '=', ' ']);
        if self.after_first_field {
            write!(self.buf, ",{key}={serialized_value}").unwrap();
        } else {
            write!(self.buf, " {key}={serialized_value}").unwrap();
            self.after_first_field = true;
        }
        self
    }

    /// Writes a field to the current line.
    ///
    /// Must be called after `tag` (or `measurement` if there's no tag).
    /// Required (there must be at least one field).
    pub fn field_float(&mut self, key: &str, value: f64) -> &mut Self {
        self.field(key, &value.to_string())
    }

    /// Writes a field to the current line.
    ///
    /// Must be called after `tag` (or `measurement` if there's no tag).
    /// Required (there must be at least one field).
    pub fn field_int(&mut self, key: &str, value: i64) -> &mut Self {
        self.field(key, &format!("{value}i"))
    }

    /// Writes a field to the current line.
    ///
    /// Must be called after `tag` (or `measurement` if there's no tag).
    /// Required (there must be at least one field).
    pub fn field_uint(&mut self, key: &str, value: u64) -> &mut Self {
        self.field(key, &format!("{value}u"))
    }

    /// Writes a field to the current line.
    ///
    /// Must be called after `tag` (or `measurement` if there's no tag).
    /// Required (there must be at least one field).
    pub fn field_string(&mut self, key: &str, value: &str) -> &mut Self {
        let escaped = escape_string(value, &['"', '\\']);
        self.field(key, &format!("\"{escaped}\""))
    }

    /// Writes a field to the current line.
    ///
    /// Must be called after `tag` (or `measurement` if there's no tag).
    /// Required (there must be at least one field).
    pub fn field_bool(&mut self, key: &str, value: bool) -> &mut Self {
        self.field(key, if value { "T" } else { "F" })
    }

    /// Writes a tag to the current line.
    ///
    /// Must be called after `field`. Required.
    pub fn timestamp(&mut self, timestamp: Timestamp) -> &mut Self {
        let nanoseconds = SystemTime::from(timestamp)
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        write!(self.buf, " {nanoseconds}").unwrap();
        self
    }

    pub fn build(self) -> LineProtocolData {
        assert!(
            self.after_first_field,
            "wrong use of the LineProtocolBuilder: at least one field is required"
        );
        LineProtocolData(self.buf)
    }
}

/// Escape a String to make it suitable for the line protocol.
///
/// See https://docs.influxdata.com/influxdb/cloud/reference/syntax/line-protocol/#special-characters.
fn escape_string<'a>(s: &'a str, chars_to_escape: &[char]) -> Cow<'a, str> {
    if s.contains(chars_to_escape) {
        // escape required, allocate a new string
        let mut escaped = String::with_capacity(s.len() + 2);
        for c in s.chars() {
            if chars_to_escape.contains(&c) {
                escaped.push('\\');
            }
            escaped.push(c);
        }
        Cow::Owned(escaped)
    } else {
        // nothing to escape, return the same string without allocating
        Cow::Borrowed(s)
    }
}

#[cfg(test)]
mod tests {
    use mockito::{Matcher, Mock, Server, ServerGuard};
    use std::time::{Duration, UNIX_EPOCH};

    use super::{Client, LineProtocolBuilder, LineProtocolData};
    use crate::influxdb2::escape_string;
    use alumet::measurement::Timestamp;

    async fn mock_influx_write(server: &mut ServerGuard, org: &str, bucket: &str, token: &str, body: &str) -> Mock {
        server
            .mock("POST", "/api/v2/write")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("org".into(), org.into()),
                Matcher::UrlEncoded("bucket".into(), bucket.into()),
                Matcher::UrlEncoded("precision".into(), "ns".into()),
            ]))
            .match_header("authorization", format!("Token {token}").as_str())
            .match_header("accept", "application/json")
            .match_header("Content-Type", "text/plain; charset=utf-8")
            .match_body(body)
            .with_status(204)
            .create_async()
            .await
    }

    struct TestedLineProtocolData {
        line: LineProtocolData,
        expected_str: &'static str,
    }

    fn get_tested_lines() -> Vec<TestedLineProtocolData> {
        let mut tested_lines = Vec::new();

        let mut builder = LineProtocolData::builder();
        builder
            .measurement("myMeasurement")
            .tag("tag1", "value1")
            .tag("tag2", "value2")
            .field_string("fieldKey", "fieldValue")
            .timestamp(Timestamp::from(UNIX_EPOCH + Duration::from_nanos(1556813561098000000)));
        let line = builder.build();

        tested_lines.push(TestedLineProtocolData {
            line,
            expected_str: r#"myMeasurement,tag1=value1,tag2=value2 fieldKey="fieldValue" 1556813561098000000"#,
        });

        let mut builder = LineProtocolData::builder();
        builder
            .measurement("myMeasurement")
            .tag("tag1", "value1")
            .tag("tag2", "value2")
            .field_string("fieldKey", "fieldValue")
            .timestamp(Timestamp::from(UNIX_EPOCH + Duration::from_nanos(1556813561098000000)));
        builder
            .measurement("measurement_without_tags")
            .field_string("fieldKey", "fieldValue")
            .field_bool("bool", true)
            .field_float("float", 123.0)
            .field_int("int", -123)
            .field_uint("uint", 123)
            .timestamp(Timestamp::from(UNIX_EPOCH + Duration::from_nanos(1556813561098000000)));
        let line = builder.build();
        tested_lines.push(TestedLineProtocolData {
            line,
            expected_str: r#"myMeasurement,tag1=value1,tag2=value2 fieldKey="fieldValue" 1556813561098000000
measurement_without_tags fieldKey="fieldValue",bool=T,float=123,int=-123i,uint=123u 1556813561098000000"#,
        });
        tested_lines
    }

    #[test]
    fn escaping() {
        assert_eq!("myMeasurement", escape_string("myMeasurement", &['\\', ' ', '=']));
        assert_eq!("with\\ space", escape_string("with space", &['\\', ' ', '=']));
        assert_eq!(
            "with\\ space\\ and\\ backslash\\\\",
            escape_string("with space and backslash\\", &['\\', ' ', '='])
        );
    }

    #[test]
    fn build_line() {
        for tested_line in get_tested_lines() {
            assert_eq!(tested_line.line.0, tested_line.expected_str);
        }
    }

    #[tokio::test]
    async fn write() {
        let mut server = Server::new_async().await;

        let token = "sometoken";
        let org = "someorg";
        let bucket = "somebucket";

        let influx_client = Client::new(server.url(), String::from(token));

        for tested_line in get_tested_lines() {
            let tested_line_write_mock =
                mock_influx_write(&mut server, org, bucket, token, tested_line.expected_str).await;

            let _ = influx_client.write(org, bucket, tested_line.line).await;
            tested_line_write_mock.assert();
        }
    }

    #[tokio::test]
    async fn test_write() {
        let mut server = Server::new_async().await;

        let token = "sometoken";
        let org = "someorg";
        let bucket = "somebucket";

        let influx_client = Client::new(server.url(), String::from(token));

        let test_write_line = mock_influx_write(&mut server, org, bucket, token, "").await;

        let _ = influx_client.test_write(org, bucket).await;
        test_write_line.assert();
    }

    #[test]
    fn verify_client() {
        let url = "http://127.0.0.1:8086";
        let token = "sometoken";
        let token_header = format!("Token {}", token);

        let influx_client = Client::new(String::from(url), String::from(token));

        assert_eq!(
            influx_client.write_url,
            format!("{}/api/v2/write", url),
            "influx write_url doesn't have the expected format when Client is created"
        );
        assert_eq!(
            influx_client.token_header, token_header,
            "influx token header doesn't have the expected format when Client is created"
        );
    }

    #[test]
    fn test_with_capacity() {
        let capacity = 100;
        let builder = LineProtocolBuilder::with_capacity(capacity);

        assert!(
            builder.buf.capacity() >= capacity,
            "Buffer capacity is less than requested"
        );
        assert_eq!(
            builder.after_first_field, false,
            "after_first_field should be false on initialization"
        );
    }
}
