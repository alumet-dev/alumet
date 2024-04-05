use std::fmt::Display;
use std::string;
use std::process::Command;
use std::collections::HashMap;
use reqwest::Result;
use std::time::Duration;
use reqwest::ClientBuilder;
use tokio;


// static apiServer: String = String::from("https://10.22.80.14:6443");

#[derive(Debug)]
struct Pod {
    node_name: String,
    metrics_associated: Vec<MetricEntry<i64>>,
}

#[derive(Debug)]
struct MetricEntry<T>{
    metric_name: String,
    arguments: HashMap<String, T>,
    value: i64,
    timestamp: i64,
}

impl MetricEntry<i64>{
    fn new(name: String, value: i64, timestamp: i64) -> MetricEntry<i64>{
        MetricEntry{
            metric_name: name,
            arguments: HashMap::new(),
            value: value,
            timestamp:timestamp
        }
    }
}

impl Pod{
    fn new(name: String, list_metric: Vec<MetricEntry<i64>>) -> Pod{
        Pod{
            node_name: name,
            metrics_associated: list_metric
        }
    }

}


// impl Display for Pod {
// }

// impl Display for MetricEntry{
// }

pub fn retrieve_token() -> String {
    let output = Command::new("sh")
                .arg("-c")
                .arg("kubectl create token alumet-reader-bis")
                .output()
                .expect("failed to execute process");
 
    let stdout_string = String::from_utf8_lossy(&output.stdout).to_string();
    println!("Output: {}", stdout_string);
    stdout_string
}

#[tokio::main]
pub async fn get_all_kubernetes_nodes(token_k8s: String, base_url: String) -> Result<()>  {
    let request_url = format!("{}/api/v1/nodes", base_url);
    println!("{}", request_url);
    let formated_header = format!("Bearer {}",token_k8s);

    let timeout = Duration::new(5, 0);
    let client = ClientBuilder::new().timeout(timeout).build()?;
    // let response = client.head(&request_url).send().await?;
    let response = client
        .get(request_url)
        .header("Authorization", formated_header)
        .send()
        .await?;

    if response.status().is_success() {
        println!("{:?} is a success!", response);
    } else {
        println!("{:?} is not a success!", response);
    }
    Ok(())
}