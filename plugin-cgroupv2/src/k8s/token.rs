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
                    .output()?;

                let token = String::from_utf8_lossy(&output.stdout);
                token.trim().to_string()
            }
            TokenRetrieval::File => {
                let mut file = File::open("/var/run/secrets/kubernetes.io/serviceaccount/token")?;
                let mut token = String::new();
                file.read_to_string(&mut token)?;
                token
            }
        };

        let mut components = token.split('.');
        let _ = components.next().ok_or(anyhow!("Could not parse the token"))?;
        let payload: serde_json::Value = serde_json::from_slice(
            &BASE64_STANDARD_NO_PAD.decode(components.next().ok_or(anyhow!("Could not parse the token"))?.trim())?,
        )?;
        let _ = components.next().ok_or(anyhow!("Could not parse the token"))?;

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
    use time::Duration;
    use tokio::time;

    use super::*;

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
