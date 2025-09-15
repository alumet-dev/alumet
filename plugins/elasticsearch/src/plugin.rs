use alumet::{
    measurement::MeasurementBuffer,
    pipeline::{
        Output,
        elements::{error::WriteError, output::OutputContext},
    },
    plugin::{
        AlumetPluginStart, ConfigTable,
        rust::{AlumetPlugin, deserialize_config, serialize_config},
    },
};
use anyhow::Context;

use crate::api;

pub struct ElasticSearchPlugin {
    config: Option<config::Config>,
}

impl AlumetPlugin for ElasticSearchPlugin {
    fn name() -> &'static str {
        "elasticsearch"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config)?;
        Ok(Box::new(ElasticSearchPlugin { config }))
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        let default = config::Config::default();
        Ok(Some(serialize_config(default)?))
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        let config = self.config.take().unwrap();

        // Parse url
        let url = &config.server_url;
        let url = reqwest::Url::parse(url).with_context(|| format!("invalid server url '{}'", &url))?;

        // Parse auth settings
        let auth = api::ApiAuthentication::try_from(config.auth).context("invalid auth config")?;

        // Create the client
        let client = api::Client::new(
            api::ConnectionSettings {
                auth,
                server_url: url,
                allow_insecure: config.allow_insecure,
            },
            api::DataSettings {
                index_prefix: config.index_prefix,
                metric_unit_as_index_suffix: config.metric_unit_as_index_suffix,
            },
        )
        .context("failed to initialize api client")?;

        // Create a template for elastic indices that will be populated by the output.
        log::info!("Creating template for Alumet indices...");
        client
            .create_index_template()
            .context("failed to create index template")?;

        // Add the output
        log::info!("Creating measurements output...");
        let out = ElasticSearchOutput { client };
        alumet.add_blocking_output("api", Box::new(out))?;

        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

struct ElasticSearchOutput {
    client: api::Client,
}

impl Output for ElasticSearchOutput {
    fn write(&mut self, measurements: &MeasurementBuffer, ctx: &OutputContext) -> Result<(), WriteError> {
        if !measurements.is_empty() {
            self.client
                .bulk_insert_measurements(measurements, ctx)
                .context("failed to send measurements")?;
        }
        Ok(())
    }
}

pub mod config {
    use std::path::PathBuf;

    use anyhow::Context;
    use serde::{Deserialize, Serialize};

    use crate::api;

    #[derive(Debug, Serialize, Deserialize)]
    pub struct Config {
        /// The url of the database instance.
        pub server_url: String,
        /// Authentication Settings (Multiple auth schemes are supported).
        pub auth: AuthConfig,
        /// Controls the use of certificate validation and hostname verification.
        /// You should think very carefully before you use this!
        pub allow_insecure: bool,
        /// The prefix added to each index (format `{index_prefix}-{metric_name}`).
        pub index_prefix: String,
        /// Controls the use of an optional suffix for each index (format `{index_prefix}-{metric_name}-{metric_unit_unique_name}`).
        pub metric_unit_as_index_suffix: bool,
    }

    #[derive(Debug, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum AuthConfig {
        ApiKey { key: String },
        Basic { user: String, password: String },
        BasicFile { file: String },
        Bearer { token: String },
    }

    impl Default for Config {
        fn default() -> Self {
            Self {
                server_url: String::from("http://localhost:9200"),
                auth: AuthConfig::Basic {
                    user: String::from("TODO"),
                    password: String::from("TODO"),
                },
                allow_insecure: false,
                index_prefix: String::from("alumet"),
                metric_unit_as_index_suffix: false,
            }
        }
    }

    impl TryFrom<AuthConfig> for api::ApiAuthentication {
        type Error = anyhow::Error;

        fn try_from(value: AuthConfig) -> Result<Self, Self::Error> {
            match value {
                AuthConfig::ApiKey { key } => Ok(api::ApiAuthentication::ApiKey { key }),
                AuthConfig::Basic { user, password } => Ok(api::ApiAuthentication::Basic { user, password }),
                AuthConfig::BasicFile { file } => {
                    let content = std::fs::read_to_string(PathBuf::from(&file))
                        .with_context(|| format!("failed to read {file}"))?;
                    if let Some((user, password)) = content.trim().split_once(':') {
                        Ok(api::ApiAuthentication::Basic {
                            user: user.to_string(),
                            password: password.to_string(),
                        })
                    } else {
                        Err(anyhow::anyhow!(
                            "file '{file}' should contain one line of the form user:password"
                        ))
                    }
                }
                AuthConfig::Bearer { token } => Ok(api::ApiAuthentication::Bearer { token }),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use alumet::{
        agent::{
            self,
            plugin::{PluginInfo, PluginSet},
        },
        pipeline,
        plugin::PluginMetadata,
    };
    use std::{
        fs::{self},
        time::Duration,
    };

    use crate::{ElasticSearchPlugin, api, plugin::config::AuthConfig};

    use super::config::Config;

    fn config_to_toml_table(config: &Config) -> toml::Table {
        toml::Value::try_from(config).unwrap().as_table().unwrap().clone()
    }

    #[test]
    fn parse_auth_config() {
        let config = r#"
            server_url = "http://localhost:5601"
            allow_insecure = true
            index_prefix = "alumet"
            metric_unit_as_index_suffix = false

            [auth.api_key]
            key = "abcd"
        "#;
        let parsed: Config = toml::from_str(config).expect("config should be valid");
        println!("{parsed:?}");
        assert!(matches!(parsed.auth, AuthConfig::ApiKey { key } if key == "abcd"));
        assert_eq!(parsed.server_url, "http://localhost:5601");
        assert_eq!(parsed.allow_insecure, true);
        assert_eq!(parsed.index_prefix, "alumet");
        assert_eq!(parsed.metric_unit_as_index_suffix, false);

        let config = r#"
            server_url = "https://192.168.1.3:5601"
            allow_insecure = false
            index_prefix = "alumet"
            metric_unit_as_index_suffix = true

            [auth.basic]
            user = "bob"
            password = "very_secure"
        "#;
        let parsed: Config = toml::from_str(config).expect("config should be valid");
        println!("{parsed:?}");
        assert!(
            matches!(parsed.auth, AuthConfig::Basic { user, password } if user == "bob" && password == "very_secure")
        );
        assert_eq!(parsed.server_url, "https://192.168.1.3:5601");
        assert_eq!(parsed.allow_insecure, false);
        assert_eq!(parsed.index_prefix, "alumet");
        assert_eq!(parsed.metric_unit_as_index_suffix, true);
    }

    #[test]
    fn try_from_basic_file_api_key_bearer() {
        // Basic File

        let tmp = tempfile::tempdir().unwrap();
        let file_path = tmp.path().join("basicfile.csv");
        fs::write(&file_path, "bob:very_secure").unwrap();

        let auth_config = AuthConfig::BasicFile {
            file: file_path.to_str().unwrap().to_string(),
        };

        let result = api::ApiAuthentication::try_from(auth_config).unwrap();
        let expected = api::ApiAuthentication::Basic {
            user: "bob".to_string(),
            password: "very_secure".to_string(),
        };
        assert_eq!(result, expected);

        // API key

        let auth_config = AuthConfig::ApiKey {
            key: "abcd".to_string(),
        };

        let result = api::ApiAuthentication::try_from(auth_config).unwrap();
        let expected = api::ApiAuthentication::ApiKey {
            key: "abcd".to_string(),
        };
        assert_eq!(result, expected);

        // Bearer

        let auth_config = AuthConfig::Bearer {
            token: "token_string".to_string(),
        };

        let result = api::ApiAuthentication::try_from(auth_config).unwrap();
        let expected = api::ApiAuthentication::Bearer {
            token: "token_string".to_string(),
        };
        assert_eq!(result, expected);
    }
}
