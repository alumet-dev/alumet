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

#[cfg(test)]
mod tests {
    use crate::mongodb2::{build_mongo_uri, convert_timestamp};
    use crate::Config;
    use alumet::measurement::Timestamp;
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn convert_timestamp_test() {
        let timestamp = Timestamp::from(UNIX_EPOCH + Duration::from_secs(1));
        assert_eq!("1000000000", convert_timestamp(timestamp));
    }

    #[test]
    fn build_mongo_uri_test() {
        let config = Config {
            host: String::from("localhost"),
            port: 27017 as u16,
            database: String::from("test1"),
            collection: String::from("test2"),
            username: None,
            password: None,
        };
        assert_eq!("mongodb://localhost:27017/", build_mongo_uri(&config));

        let config = Config {
            host: String::from("localhost"),
            port: 27017 as u16,
            database: String::from("test1"),
            collection: String::from("test2"),
            username: Some(String::from("user")),
            password: Some(String::from("password")),
        };
        assert_eq!("mongodb://user:password@localhost:27017/", build_mongo_uri(&config));
    }
}
