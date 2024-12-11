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

pub struct Token {
    retrieval: TokenRetrieval,
    data: Arc<RwLock<TokenData>>,
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
        }
    }

    /// Check if the token' lifetime has reached its expiration or not.
    async fn is_valid(&self) -> bool {
        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("ERROR : Failed to get the current time since UNIX_EPOCH")
            .as_secs();

        let data_locked = self.data.read().await;
        let Some(exp) = data_locked.expiration_time else {
            return false;
        };

        data_locked.value.is_some() && now_secs < exp
    }

    /// Returns the current value of the token if it is still alive or
    /// it's refreshed value if not.
    pub async fn get_value(&self) -> anyhow::Result<String> {
        match self.is_valid().await {
            true => {
                let data_locked = self.data.read().await;
                match &data_locked.value {
                    Some(value) => Ok(value.clone()),
                    None => Err(anyhow!("ERROR : Could not read the token's value")),
                }
            }
            false => match self.refresh().await {
                Ok(token) => Ok(token),
                Err(e) => Err(anyhow!("ERROR : Failed to refresh token: {}", e)),
            },
        }
    }

    /// Retrieves the k8s API token using either a kubectl command
    /// or by reading  the service account token's file.
    async fn refresh(&self) -> anyhow::Result<String> {
        let token: String = match self.retrieval {
            TokenRetrieval::Kubectl => {
                let output = Command::new("kubectl")
                    .args(["create", "token", "alumet-reader", "-n", "alumet"])
                    .output()
                    .context("ERROR : kubectl command execution")?;

                String::from_utf8(output.stdout)
                    .context("ERROR : UTF-8 output conversion")?
                    .trim()
                    .to_string()
            }

            TokenRetrieval::File => {
                let mut file = File::open("/var/run/secrets/kubernetes.io/serviceaccount/token")
                    .context("ERROR : opening token file")?;
                let mut token = String::new();
                file.read_to_string(&mut token).expect("ERROR : Reading token file");
                token
            }
        };

        let mut components = token.split('.');
        let payload: &str = components
            .next()
            .context("ERROR : Missing payload, token can be parsed")?;

        let decoded_payload: Vec<u8> = BASE64_STANDARD_NO_PAD
            .decode(payload.trim())
            .context("ERROR : payload decoding")?;

        let payload_json: serde_json::Value =
            serde_json::from_slice(&decoded_payload).context("ERROR : JSON payload parsing")?;

        let exp = payload_json
            .get("exp")
            .and_then(|exp| exp.as_u64())
            .context("ERROR : Missing or invalid 'exp' field in token")?;

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
    use mockito::mock;
    use time::Duration;
    use tokio::time;

    // Test `refresh` function with a kubectl failure simulation
    #[tokio::test]
    async fn test_refresh_1() {
        let _m = mock("POST", "/api/alumet/kubernetes.io/serviceaccounts/token")
            .with_status(1)
            .with_body("Error occurred")
            .create();

        let token= Token::new(TokenRetrieval::Kubectl);
        let result = token.refresh().await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "ERROR : kubectl command execution");
    }

    // Test `refresh` function with a kubectl utf8 error
    #[tokio::test]
    async fn test_refresh_2() {
        let _m = mock("POST", "/api/alumet/kubernetes.io/serviceaccounts/token")
            .with_status(200)
            .with_body(b"\xFF\xFE".to_vec())
            .create();

        let token = Token::new(TokenRetrieval::Kubectl);
        let result = token.refresh().await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "ERROR : kubectl command execution");
    }

    // Test `refresh` function with a file opening error
    #[tokio::test]
    async fn test_refresh_3() {
        let token = Token::new(TokenRetrieval::File);
        let result = token.refresh().await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "ERROR : opening token file");
    }

    // Test `refresh` function with a file reading error
    #[tokio::test]
    async fn test_refresh_4() {
        let _m = mock("GET", "/var/run/secrets/kubernetes.io/serviceaccount/token")
            .with_status(500)
            .create();

        let token = Token::new(TokenRetrieval::File);
        let result = token.refresh().await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "ERROR : opening token file");
    }

    // Test `refresh` function with a missing payload
    #[tokio::test]
    async fn test_refresh_5() {
        let token = Token::new(TokenRetrieval::Kubectl);
        let result = token.refresh().await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "ERROR : kubectl command execution");
    }

    // Test `get_value` function to get token with valid value
    #[tokio::test]
    async fn test_get_value_1() {
        let token: Token = Token::new(TokenRetrieval::File);

        {
            let mut data = token.data.write().await;
            data.value = Some("valid_token".to_string());
            data.expiration_time = Some(SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() + 3600);
        }

        let result = token.get_value().await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "valid_token");
    }

    // Test `get_value` function to get token with invalid value
    #[tokio::test]
    async fn test_get_value_2() {
        let token: Token = Token::new(TokenRetrieval::File);
        {
            let mut data: tokio::sync::RwLockWriteGuard<'_, TokenData> = token.data.write().await;
            data.value = None;
            data.expiration_time = None;
        }

        let result = token.get_value().await;
        assert!(result.is_err());
        //assert_eq!(result.unwrap_err().to_string(), "ERROR : invalid token");
    }

    // Test `get_value` function to get token with invalid value
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
}
