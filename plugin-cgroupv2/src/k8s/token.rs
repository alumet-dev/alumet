use std::{
    fs::File,
    io::Read,
    process::Command,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::anyhow;
use base64::{prelude::BASE64_STANDARD_NO_PAD, Engine};
use tokio::sync::RwLock;

use super::plugin::TokenRetrieval;

pub struct Token {
    retrieval: TokenRetrieval,
    data: Arc<RwLock<TokenData>>,
}

/// Private structure that holds the JWT token data.
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
                    .map_err(|e| anyhow!("Failed to execute kubectl command: {}", e))?;

                if !output.status.success() {
                    return Err(anyhow!(
                        "kubectl raised an error: {}",
                        String::from_utf8_lossy(&output.stderr)
                    ));
                }

                String::from_utf8_lossy(&output.stdout).trim().to_string()
            }
            TokenRetrieval::File => {
                let mut file = File::open("/var/run/secrets/kubernetes.io/serviceaccount/token")
                    .map_err(|e| anyhow!("Failed to open service account token file: {}", e))?;
                let mut token = String::new();
                file.read_to_string(&mut token)
                    .map_err(|e| anyhow!("Failed to read service account token file: {}", e))?;
                token
            }
        };

        let mut components = token.split('.');
        let payload = components
            .next()
            .ok_or_else(|| anyhow!("Could not parse the token, payload is missing"))?;

        let decoded_payload = BASE64_STANDARD_NO_PAD
            .decode(payload.trim())
            .map_err(|e| anyhow!("Failed to decode payload: {}", e))?;

        let payload_json: serde_json::Value =
            serde_json::from_slice(&decoded_payload).map_err(|e| anyhow!("Failed to parse payload as JSON: {}", e))?;

        let exp = match payload_json.get("exp") {
            Some(exp) => exp
                .as_u64()
                .ok_or_else(|| anyhow!("Expiration time 'exp' is not a valid u64"))?,
            None => {
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

// ------------------ //
// --- UNIT TESTS --- //
// ------------------ //
#[cfg(test)]
mod tests {
    use super::*;
    use mockito::mock;
    use time::Duration;
    use tokio::time;

    // Test `refresh` function with a kubectl simulated
    #[tokio::test]
    async fn test_refresh_1() {
        let _m = mock("POST", "/api/v1/namespaces/alumet/serviceaccounts/alumet-reader/token")
            .with_status(10)
            .with_body("eyJhbG.eyJ")
            .create();

        let retrieval = TokenRetrieval::Kubectl;
        let token = Token::new(retrieval);
        let result = token.refresh().await;

        //assert!(result.is_ok());
        let data = token.data.read().await;
        //assert!(data.value.is_some());
        //assert!(data.expiration_time.is_some());
    }

    // Test `refresh` function with a file
    #[tokio::test]
    async fn test_refresh_2() {
        let _m = mock("GET", "/var/run/secrets/kubernetes.io/serviceaccount/token")
            .with_status(10)
            .with_body("eyJh.bGeyJ")
            .create();

        let retrieval = TokenRetrieval::File;
        let token = Token::new(retrieval);
        let result = token.refresh().await;

        //assert!(result.is_ok());
        let data = token.data.read().await;
        //assert!(data.value.is_some());
        //assert!(data.expiration_time.is_some());
    }

    // Test `get_value` function to get token with valid value
    #[tokio::test]
    async fn test_get_value_1() {
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

    // Test `get_value` function to get token with invalid value
    #[tokio::test]
    async fn test_get_value_2() {
        let retrieval = TokenRetrieval::File;
        let token = Token::new(retrieval);

        {
            // Expired token
            let mut data = token.data.write().await;
            data.value = Some("expired_token".to_string());
            data.expiration_time = Some(SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs());
        }

        let result = token.get_value().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_is_valid() {
        let token: Token = Token::new(TokenRetrieval::File);
        assert!(!token.is_valid().await);

        {
            // Expand the expiration of the token to make it still valid.
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
            // Decrease the expiration of the token to make it invalid.
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
