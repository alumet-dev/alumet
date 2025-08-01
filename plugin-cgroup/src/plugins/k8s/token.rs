use std::{
    process::Command,
    str::FromStr,
    sync::{Arc, RwLock},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, Context};
use base64::{DecodeError, Engine};
use serde::{Deserialize, Serialize};
use thiserror::Error;

const DEFAULT_SECRET_TOKEN_PATH: &str = "/var/run/secrets/kubernetes.io/serviceaccount/token";
const DEFAULT_SERVICE_ACCOUNT: &str = "alumet-reader";
const DEFAULT_NAMESPACE: &str = "alumet";

/// A [`TokenRetrieval`] that has been modified to simplify the configuration file.
///
/// # Examples
/// ```toml
/// # run 'kubectl create token'
/// token_retrieval = "kubectl"
///
/// # read /var/run/secrets/kubernetes.io/serviceaccount/token
/// token_retrieval = "file"
///
/// # custom file
/// token_retrieval.file = "/path/to/token"
///
/// # custom kubectl
/// token_retrieval.kubectl = {
///     service_account = "alumet-account"
///     namespace = "alumet-ns"
/// }
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TokenRetrievalConfig {
    Kubectl {
        #[serde(default = "default_sa")]
        service_account: String,
        #[serde(default = "default_ns")]
        namespace: String,
    },
    File(String),
    #[serde(untagged)]
    Simple(SimpleRetrievalMethod),
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SimpleRetrievalMethod {
    Kubectl,
    File,
    Auto,
}

impl From<TokenRetrievalConfig> for TokenRetrieval {
    fn from(value: TokenRetrievalConfig) -> Self {
        match value {
            TokenRetrievalConfig::Simple(SimpleRetrievalMethod::Kubectl) => Self::Kubectl {
                service_account: default_sa(),
                namespace: default_ns(),
            },
            TokenRetrievalConfig::Simple(SimpleRetrievalMethod::File) => {
                Self::File(DEFAULT_SECRET_TOKEN_PATH.to_string())
            }
            TokenRetrievalConfig::Simple(SimpleRetrievalMethod::Auto) => {
                if let Ok(true) = std::fs::exists(DEFAULT_SECRET_TOKEN_PATH) {
                    Self::from(TokenRetrievalConfig::Simple(SimpleRetrievalMethod::File))
                } else {
                    Self::from(TokenRetrievalConfig::Simple(SimpleRetrievalMethod::Kubectl))
                }
            }
            TokenRetrievalConfig::Kubectl {
                service_account,
                namespace,
            } => Self::Kubectl {
                service_account,
                namespace,
            },
            TokenRetrievalConfig::File(path) => Self::File(path),
        }
    }
}

/// A way of obtaining a Kubernetes token.
#[derive(Clone, PartialEq, Debug)]
pub enum TokenRetrieval {
    Kubectl { service_account: String, namespace: String },
    File(String),
}

// default providers for serde
fn default_sa() -> String {
    DEFAULT_SERVICE_ACCOUNT.to_string()
}

fn default_ns() -> String {
    DEFAULT_NAMESPACE.to_string()
}

impl TokenRetrieval {
    /// Reads the token's content from a `kubectl` command or a secret token file.
    pub fn read(&self) -> anyhow::Result<String> {
        match self {
            TokenRetrieval::Kubectl {
                service_account,
                namespace,
            } => {
                log::debug!("running: kubectl create token {service_account} -n {namespace}");
                let output = Command::new("kubectl")
                    .args(["create", "token", service_account, "-n", namespace])
                    .output()
                    .context("failed to run kubectl command")?;

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    match output.status.code() {
                        Some(code) => return Err(anyhow!("kubectl exited with status {code}\n{stderr}")),
                        None => return Err(anyhow!("kubectl terminated by signal\n{stderr}")),
                    }
                }

                match String::from_utf8(output.stdout) {
                    Ok(content) => Ok(content.trim().to_string()),
                    Err(e) => Err(anyhow::Error::from(e).context("invalid kubectl output")),
                }
            }
            TokenRetrieval::File(path) => {
                std::fs::read_to_string(path).with_context(|| format!("failed to read token from file {path:?}"))
            }
        }
    }
}

/// Kubernetes token and way to retrieve it.
#[derive(Debug, Clone)]
pub struct Token {
    /// Way to obtain a token, in file or with Kubectl command.
    retrieval: TokenRetrieval,
    /// Thread safe token data.
    data: Arc<RwLock<TokenData>>,
}

/// Private structure that holds the JWT token data.
#[derive(Debug, Default)]
struct TokenData {
    /// Optional expiration time, in POSIX seconds.
    expiration_time: Option<u64>,
    /// Before the first retrieval, there is no token.
    value: Option<String>,
}

impl TokenData {
    fn is_valid(&self) -> bool {
        self.value.is_some() && self.is_alive()
    }

    /// Checks whether the token's lifetime is still running.
    ///
    /// Token expired = not alive.
    fn is_alive(&self) -> bool {
        match self.expiration_time {
            None => {
                // the token never expires, it is always valid
                true
            }
            Some(deadline) => {
                // get the current time in POSIX seconds
                let now_secs = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("current time should never be before UNIX_EPOCH")
                    .as_secs();

                // check that the expiration deadline has not come yet
                now_secs < deadline
            }
        }
    }

    /// Returns the token data, if it is valid.
    fn get_valid(&self) -> Option<&String> {
        self.value.as_ref().filter(|_| self.is_alive())
    }
}

#[derive(Debug, Error)]
pub enum TokenParseError {
    #[error("invalid token format")]
    InvalidFormat,
    #[error("invalid token payload")]
    InvalidPayload(#[from] InvalidPayloadError),
}

#[derive(Debug, Error)]
pub enum InvalidPayloadError {
    #[error("base64 decoding failed")]
    Decoding(#[from] DecodeError),
    #[error("json parsing failed")]
    Parsing(#[from] serde_json::Error),
    #[error("exp field is invalid: expected a u64, got {0}")]
    ExpirationTime(serde_json::Value),
}

impl FromStr for TokenData {
    type Err = TokenParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // A JWT token is made of 3 parts: header, payload, signature.
        let mut components = s.split('.');
        let _header = components.next().ok_or(TokenParseError::InvalidFormat)?;
        let payload = components.next().ok_or(TokenParseError::InvalidFormat)?;
        let _signature = components.next().ok_or(TokenParseError::InvalidFormat)?;

        // Check that there's no mort part.
        if components.next().is_some() {
            return Err(TokenParseError::InvalidFormat);
        }

        // Decode the payload
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(payload)
            .map_err(InvalidPayloadError::from)?;

        let payload: serde_json::Map<String, serde_json::Value> =
            serde_json::from_slice(&payload).map_err(InvalidPayloadError::from)?;

        // Get the expiration deadline from the payload.
        let expiration_time = match payload.get("exp") {
            Some(exp) => {
                let exp = exp
                    .as_u64()
                    .ok_or(InvalidPayloadError::ExpirationTime(exp.to_owned()))?;
                Some(exp)
            }
            None => {
                // no expiration time, that's fine
                None
            }
        };
        Ok(Self {
            expiration_time,
            value: Some(s.to_string()),
        })
    }
}

impl Token {
    pub fn new(retrieval: TokenRetrieval) -> Self {
        Self {
            retrieval,
            data: Arc::new(RwLock::new(TokenData {
                expiration_time: None,
                value: None,
            })),
        }
    }

    #[cfg(test)]
    pub fn with_kubectl() -> Self {
        Self::new(TokenRetrievalConfig::Simple(SimpleRetrievalMethod::Kubectl).into())
    }

    #[cfg(test)]
    pub fn with_file(path: String) -> Self {
        Self::new(TokenRetrieval::File(path))
    }

    #[cfg(test)]
    pub fn with_default_file() -> Self {
        Self::new(TokenRetrieval::File(DEFAULT_SECRET_TOKEN_PATH.into()))
    }

    /// Returns the current value of the token if it is still alive,
    /// or its refreshed value if not.
    pub fn get_value(&self) -> anyhow::Result<String> {
        {
            let data = self.data.read().unwrap();
            if let Some(v) = data.get_valid() {
                return Ok(v.to_owned());
            }
        } // unlock

        // refresh needed
        self.refresh()
    }

    fn refresh(&self) -> anyhow::Result<String> {
        let token_content = self.retrieval.read().context("failed to read token")?;
        let data = TokenData::from_str(&token_content).context("failed to parse token")?;
        *self.data.write().unwrap() = data;
        Ok(token_content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use time::Duration;
    use tokio::time;

    /// Test `get_value` function to get token with valid value
    #[test]
    fn test_get_value_with_valid_value() {
        let token = Token::with_default_file();

        {
            // Expand the expiration of the token to make it still valid.
            let mut data = token.data.write().unwrap();
            data.value = Some("valid_token".to_string());
            data.expiration_time = Some(SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() + 3600);
        }

        let result = token.get_value();
        assert_eq!(result.unwrap(), "valid_token");
    }

    /// Test `get_value` function to returns error when value is none
    #[test]
    fn test_get_value_with_invalid_value() {
        let token = Token::with_default_file();

        {
            // Expand the expiration of the token to make it still valid.
            let mut data = token.data.write().unwrap();
            data.value = None;
            data.expiration_time = Some(SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() + 3600);
        }

        let result = token.get_value();
        assert!(result.is_err());
    }

    /// Test `is_valid` function
    #[test]
    fn test_is_valid() {
        let mut token_data = TokenData::default();
        assert!(!token_data.is_valid());

        // 10s in future
        token_data.expiration_time = Some(
            SystemTime::now()
                .checked_add(Duration::from_secs(10))
                .unwrap()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        );
        token_data.value = Some("abcde".to_string());

        assert!(token_data.is_valid());
        assert_eq!(token_data.get_valid(), Some(&"abcde".to_string()));

        // 10s in past
        token_data.expiration_time = Some(
            SystemTime::now()
                .checked_sub(Duration::from_secs(10))
                .unwrap()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        );
        assert!(!token_data.is_valid());
        assert!(token_data.get_valid().is_none());
    }

    /// Test `refresh` function with valid file and token
    #[test]
    fn test_refresh_with_valid_token() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("test-alumet-plugin-k8s/var_1/");

        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }

        let dir = root.join("run/secrets/kubernetes.io/serviceaccount/");
        std::fs::create_dir_all(&dir).unwrap();

        // HEADER = { "alg": "HS256", "typ": "JWT" }
        // PAYLOAD = { "sub": "1234567890", "exp": 4102444800, "name": "T3st1ng T0k3n" }
        // SIGNATURE = { HMACSHA256(base64UrlEncode(header) + "." +  base64UrlEncode(payload), signature) }
        let content = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwiZXhwIjo0MTAyNDQ0ODAwLCJuYW1lIjoiVDNzdDFuZyBUMGszbiJ9.3vho4u0hx9QobMNbpDPvorWhTHsK9nSg2pZAGKxeVxA";
        let path = dir.join("token_1");

        std::fs::write(&path, content).unwrap();

        let token = Token::with_file(path.to_str().unwrap().to_owned());
        let result = token.refresh();

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), content);

        let data = token.data.read().unwrap();
        assert_eq!(data.value, Some(content.to_string()));
        assert_eq!(data.expiration_time, Some(4102444800));
    }

    /// Test `refresh` function with missing exp field token
    #[test]
    fn test_refresh_with_missing_exp() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("test-alumet-plugin-k8s/var_2/");

        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }

        let dir = root.join("run/secrets/kubernetes.io/serviceaccount/");
        std::fs::create_dir_all(&dir).unwrap();

        // HEADER = { "alg": "HS256", "typ": "JWT" }
        // PAYLOAD = { "sub": "1234567890", "name": "T3st1ng T0k3n", "iat": 1516239022 }
        // SIGNATURE = { HMACSHA256(base64UrlEncode(header) + "." +  base64UrlEncode(payload), signature) }
        let content =
            "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IlQzc3QxbmcgVDBrM24iLCJpYXQiOjE1MTYyMzkwMjJ9.3vho4u0hx9QobMNbpDPvorWhTHsK9nSg2pZAGKxeVxA";
        let path = dir.join("token_2");

        std::fs::write(&path, content).unwrap();

        let token = Token::with_file(path.to_str().unwrap().to_owned());
        let result = token.refresh();
        assert!(result.is_ok());
        assert!(token.data.read().unwrap().is_alive());
    }

    fn serialize_toml_value(value: &impl Serialize) -> Result<String, toml::ser::Error> {
        let mut dst = String::new();
        let ser = toml::ser::ValueSerializer::new(&mut dst);
        let () = serde::Serialize::serialize(value, ser)?;
        Ok(dst)
    }

    fn deserialize_toml_value<'de, T: Deserialize<'de>>(value: &str) -> Result<T, toml::de::Error> {
        let de = toml::de::ValueDeserializer::new(value);
        T::deserialize(de)
    }

    #[test]
    fn test_config_serialize_simple() {
        assert_eq!(
            serialize_toml_value(&TokenRetrievalConfig::Simple(SimpleRetrievalMethod::Auto)).unwrap(),
            "\"auto\""
        );
        assert_eq!(
            serialize_toml_value(&TokenRetrievalConfig::Simple(SimpleRetrievalMethod::File)).unwrap(),
            "\"file\""
        );
        assert_eq!(
            serialize_toml_value(&TokenRetrievalConfig::Simple(SimpleRetrievalMethod::Kubectl)).unwrap(),
            "\"kubectl\""
        );
    }

    #[test]
    fn test_config_serialize_advanced() {
        assert_eq!(
            serialize_toml_value(&TokenRetrievalConfig::File("/a/b".to_string())).unwrap(),
            "{ file = \"/a/b\" }"
        );
        assert_eq!(
            serialize_toml_value(&TokenRetrievalConfig::Kubectl {
                service_account: "svac".to_string(),
                namespace: "ns".to_string()
            })
            .unwrap(),
            "{ kubectl = { service_account = \"svac\", namespace = \"ns\" } }"
        );
    }

    #[test]
    fn test_config_parsing_simple() {
        let config: TokenRetrievalConfig = deserialize_toml_value("\"kubectl\"").unwrap();
        assert_eq!(config, TokenRetrievalConfig::Simple(SimpleRetrievalMethod::Kubectl));

        let r = TokenRetrieval::from(config);
        assert!(
            matches!(
                &r,
                TokenRetrieval::Kubectl {
                    service_account,
                    namespace
                }
                if service_account == DEFAULT_SERVICE_ACCOUNT && namespace == DEFAULT_NAMESPACE
            ),
            "{r:?}"
        );

        let config: TokenRetrievalConfig = deserialize_toml_value("\"file\"").unwrap();
        assert_eq!(config, TokenRetrievalConfig::Simple(SimpleRetrievalMethod::File));

        let r = TokenRetrieval::from(config);
        assert!(
            matches!(
                &r,
                TokenRetrieval::File(path) if path == DEFAULT_SECRET_TOKEN_PATH
            ),
            "{r:?}"
        );
    }

    #[test]
    fn test_config_parsing_advanced_kubectl() {
        let config: TokenRetrievalConfig =
            deserialize_toml_value(r#"{ kubectl = { service_account = "svac", namespace = "ns"} }"#).unwrap();
        assert_eq!(
            config,
            TokenRetrievalConfig::Kubectl {
                service_account: String::from("svac"),
                namespace: String::from("ns")
            }
        );

        let r = TokenRetrieval::from(config);
        assert!(
            matches!(
                &r,
                TokenRetrieval::Kubectl {
                    service_account,
                    namespace
                }
                if service_account == "svac" && namespace == "ns"
            ),
            "{r:?}"
        );
    }

    #[test]
    fn test_config_parsing_advanced_file() -> anyhow::Result<()> {
        let config: TokenRetrievalConfig = deserialize_toml_value(r#"{ file = "/a/b" }"#).unwrap();
        assert_eq!(config, TokenRetrievalConfig::File(String::from("/a/b")));

        let r = TokenRetrieval::from(config);
        assert!(
            matches!(
                &r,
                TokenRetrieval::File(path) if path == "/a/b"
            ),
            "{r:?}"
        );
        Ok(())
    }
}
