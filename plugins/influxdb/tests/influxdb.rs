pub mod fakeplugin;

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use mockito::{Matcher, Mock, Server, ServerGuard};

    use plugin_influxdb::{AttributeAs, Config, InfluxDbPlugin};

    use crate::fakeplugin::TestsPlugin;

    use alumet::{
        agent::{
            self,
            plugin::{PluginInfo, PluginSet},
        },
        measurement::{MeasurementBuffer, MeasurementPoint, Timestamp, WrappedMeasurementValue},
        pipeline::naming::OutputName,
        plugin::PluginMetadata,
        resources::{Resource, ResourceConsumer},
        test::{RuntimeExpectations, runtime::OutputCheckInputContext},
    };

    fn mock_influx_write(server: &mut ServerGuard, org: &str, bucket: &str, token: &str, body: &str) -> Mock {
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
            .create()
    }

    #[test]
    fn write_measure() {
        let mut server = Server::new();

        let token = "sometoken";
        let org = "someorg";
        let bucket = "somebucket";

        let test_write_mock = mock_influx_write(&mut server, org, bucket, token, "");

        let measure_timestamp_ns = 818254800000000000;
        let measure_timestamp = UNIX_EPOCH + Duration::from_nanos(measure_timestamp_ns);

        let measure_write_mock = mock_influx_write(
            &mut server,
            org,
            bucket,
            token,
            format!(
                "dumb,resource_kind=local_machine,resource_consumer_kind=local_machine value=10u {}",
                measure_timestamp_ns
            )
            .as_str(),
        );

        let mut plugins = PluginSet::new();

        let source_config = Config {
            host: server.url(),
            token: String::from(token),
            org: String::from(org),
            bucket: String::from(bucket),
            attributes_as: AttributeAs::Field,
            attributes_as_tags: None,
            attributes_as_fields: None,
        };
        plugins.add_plugin(PluginInfo {
            metadata: PluginMetadata::from_static::<InfluxDbPlugin>(),
            enabled: true,
            config: Some(config_to_toml_table(&source_config)),
        });
        plugins.add_plugin(PluginInfo {
            metadata: PluginMetadata::from_static::<TestsPlugin>(),
            enabled: true,
            config: None,
        });

        let make_input = move |ctx: &mut OutputCheckInputContext| -> MeasurementBuffer {
            let metric = ctx.metrics().by_name("dumb").expect("metric should exist").0;
            let mut m = MeasurementBuffer::new();
            let test_point = MeasurementPoint::new_untyped(
                Timestamp::from(measure_timestamp),
                metric,
                Resource::LocalMachine,
                ResourceConsumer::LocalMachine,
                WrappedMeasurementValue::U64(10),
            );
            m.push(test_point);
            m
        };
        let check_output = move || {
            test_write_mock.assert();
            measure_write_mock.assert();
        };

        let runtime_expectations =
            RuntimeExpectations::new().test_output(OutputName::from_str("influxdb", "out"), make_input, check_output);

        let agent = agent::Builder::new(plugins)
            .with_expectations(runtime_expectations)
            .build_and_start()
            .unwrap();

        agent.wait_for_shutdown(Duration::from_secs(2)).unwrap();
    }
    fn config_to_toml_table(config: &Config) -> toml::Table {
        toml::Value::try_from(config).unwrap().as_table().unwrap().clone()
    }
}
