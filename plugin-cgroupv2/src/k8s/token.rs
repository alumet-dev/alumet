use std::{
    fs::File,
    io::Read,
    process::Command,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, Context};
use base64::{prelude::BASE64_STANDARD_NO_PAD, Engine};
use tokio::sync::RwLock;

use super::plugin::TokenRetrieval;

/// Kubernetes token and way to retrieve it.
#[derive(Debug)]
pub struct Token {
    /// Way to obtain a token, in file or with Kubectl command.
    retrieval: TokenRetrieval,
    /// Thread safe token data.
    data: Arc<RwLock<TokenData>>,
    /// Public path for a token if we store it in a file.
    pub path: Option<String>,
}

/// Private structure that holds the JWT token data.
#[derive(Debug)]
struct TokenData {
    /// Optional expiration time, in POSIX seconds.
    expiration_time: Option<u64>,
    /// Before the first retrieval, there is no token.
    value: Option<String>,
}

impl Token {
    pub fn new(token_retrieval: TokenRetrieval) -> Self {
        Self {
            retrieval: token_retrieval,
            data: Arc::new(RwLock::new(TokenData {
                expiration_time: None,
                value: None,
            })),
            path: None,
        }
    }

    /// Check if the token' lifetime has reached its expiration or not.
    async fn is_valid(&self) -> bool {
        let now_secs = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(ts) => ts.as_secs(),
            Err(_) => return false,
        };

        // Lock the data for the entire check.
        let data_locked = self.data.read().await;
        let Some(exp) = data_locked.expiration_time else {
            return false;
        };

        data_locked.value.is_some() && now_secs < exp
    }

    /// Returns the current value of the token if it is still alive or
    /// it's refreshed value if not.
    pub async fn get_value(&self) -> anyhow::Result<String> {
        if self.is_valid().await {
            return match &self.data.read().await.value {
                Some(value) => Ok(value.clone()),
                None => Err(anyhow!("could no read the token's value")),
            };
        }

        self.refresh().await
    }

    /// Retrieves the k8s API token using either a kubectl command
    /// or by reading  the service account token's file.
    async fn refresh(&self) -> anyhow::Result<String> {
        let token = match self.retrieval {
            TokenRetrieval::Kubectl => {
                let output = Command::new("kubectl")
                    .args(["create", "token", "alumet-reader", "-n", "alumet"])
                    .output()
                    .context("Kubectl command execution")?;

                if !output.status.success() {
                    return Err(anyhow!(
                        "kubectl raised an error: {}",
                        String::from_utf8_lossy(&output.stderr)
                    ));
                }

                let token = String::from_utf8_lossy(&output.stdout);
                token.trim().to_string()
            }

            TokenRetrieval::File => {
                let token_path = match &self.path {
                    Some(path_value) => path_value.clone(),
                    None => "/var/run/secrets/kubernetes.io/serviceaccount/token".to_string(),
                };

                let mut file = File::open(token_path).context("Could not retrieve the file")?;
                let mut token = String::new();
                file.read_to_string(&mut token)?;
                token
            }
        };

        let mut components = token.split('.');
        let _ = components
            .next()
            .ok_or(anyhow!("Could not parse the token, the header is missing"))?;
        let payload: serde_json::Value = serde_json::from_slice(
            &BASE64_STANDARD_NO_PAD.decode(
                components
                    .next()
                    .ok_or(anyhow!("Could not parse the token, the payload is missing"))?
                    .trim(),
            )?,
        )?;

        let _ = components
            .next()
            .ok_or(anyhow!("Could not parse the token, the signature is missing"))?;

        if components.next().is_some() {
            return Err(anyhow!("Token has too many components"));
        }

        // Extract 'exp' from the payload
        let exp = match payload.get("exp") {
            Some(exp) => exp.as_u64().unwrap_or_default(),
            None => {
                // If the field exp is missing from the JWT, we need to reset it to None
                // in the token's data.
                let mut new_data = self.data.write().await;
                new_data.expiration_time = None;

                return Err(anyhow!("Could not extract the 'exp' field from the token"));
            }
        };

        {
            let mut new_data = self.data.write().await;
            new_data.expiration_time = Some(exp);
            new_data.value = Some(token.clone());
        }

        Ok(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use time::Duration;
    use tokio::time;

    // Test `get_value` function to get token with valid value
    #[tokio::test]
    async fn test_get_value_with_valid_value() {
        let retrieval = TokenRetrieval::File;
        let token = Token::new(retrieval);

        {
            // Expand the expiration of the token to make it still valid.
            let mut data = token.data.write().await;
            data.value = Some("valid_token".to_string());
            data.expiration_time = Some(SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() + 3600);
        }

        let result = token.get_value().await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "valid_token");
    }

    // Test `get_value` function to returns error when value is none
    #[tokio::test]
    async fn test_get_value_with_invalid_value() {
        let retrieval = TokenRetrieval::File;
        let token = Token::new(retrieval);

        {
            // Expand the expiration of the token to make it still valid.
            let mut data = token.data.write().await;
            data.value = None;
            data.expiration_time = Some(SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() + 3600);
        }

        let result = token.get_value().await;
        assert!(result.is_err());
    }

    // Test `is_valid` function
    #[tokio::test]
    async fn test_is_valid() {
        let token: Token = Token::new(TokenRetrieval::File);
        assert!(!token.is_valid().await);
        {
            let mut new_data = token.data.write().await;
            (*new_data).expiration_time = Some(
                SystemTime::now()
                    .checked_add(Duration::from_secs(10))
                    .unwrap()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            );
            (*new_data).value = Some("abcde".to_string());
        }

        assert!(token.is_valid().await);
        {
            let mut new_data = token.data.write().await;
            (*new_data).expiration_time = Some(
                SystemTime::now()
                    .checked_sub(Duration::from_secs(10))
                    .unwrap()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            );
        }

        assert!(!token.is_valid().await);
    }

    // Test `refresh` function with valid file and token
    #[tokio::test]
    async fn test_refresh_with_valid_token() {
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

        let mut token = Token::new(TokenRetrieval::File);
        token.path = Some(path.to_str().unwrap().to_owned());
        let result = token.refresh().await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), content);

        let data = token.data.read().await;
        assert_eq!(data.value, Some(content.to_string()));
        assert_eq!(data.expiration_time, Some(4102444800));
    }

    // Test `refresh` function with missing exp field token
    #[tokio::test]
    async fn test_refresh_with_missing_exp() {
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

        let mut token = Token::new(TokenRetrieval::File);
        token.path = Some(path.to_str().unwrap().to_owned());
        let result = token.refresh().await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Could not extract the 'exp' field from the token"),);

        let data = token.data.read().await;
        assert_eq!(data.expiration_time, None);
        assert_eq!(data.value, None);
    }
}
