use alumet::measurement::Timestamp;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::Config;

pub fn build_mongo_uri(config: &Config) -> String {
    if config.username.is_some() {
        return format!(
            "mongodb://{}:{}@{}:{}/",
            config.username.as_ref().unwrap(),
            config.password.as_ref().unwrap(),
            config.host,
            config.port
        );
    }
    format!("mongodb://{}:{}/", config.host, config.port)
}

pub fn convert_timestamp(timestamp: Timestamp) -> String {
    let nanoseconds = SystemTime::from(timestamp)
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{nanoseconds}")
}
