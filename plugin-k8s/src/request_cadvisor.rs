use std::fmt::Display;
use std::string::String;
use std::process::Command;
use reqwest::{ClientBuilder, Error};
use std::time::Duration;
use tokio;
use serde_json::Value;
use anyhow::{anyhow, Context};
use std::fs::File;
use regex::Regex;
use std::{collections::HashMap, str::FromStr};

use crate::parsing_prometheus::PrometheusMetric;


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
pub async fn get_all_kubernetes_nodes(token_k8s: &String, base_url: &String) -> Result<Vec<String>, reqwest::Error>  {
    let request_url = format!("{}/api/v1/nodes", base_url);
    println!("{}", request_url);
    let formated_header = format!("Bearer {}",token_k8s);

    let timeout = Duration::new(5, 0);
    let client = ClientBuilder::new().timeout(timeout).build()?;
    // let response = client.head(&request_url).send().await?;
    let response_to_send = client
        .get(request_url)
        .header("Authorization", formated_header).send().await;

    match response_to_send {
        Ok(response) => {
            // let jsonTransformed: Result<_> = response.json().await?;
            let jsonTransformed: serde_json::Value = response.json().await?;
            let mut list_node: Vec<String> = Vec::new();
            if let Some(liItem) = jsonTransformed["items"].as_array(){
                for item in liItem {
                    if let Some(name) = item["metadata"]["name"].as_str() {
                        // Append the node name to the list
                        list_node.push(name.to_string());
                    } else {
                        println!("Node name is not a valid string.");
                    }
                }
                
            }
            return Ok(list_node);
        }
        Err(err) => {

            
            ///////////////////////
            /// 
            /// 
            let file = File::open("/home/cyprien/code/alumet/plugin-k8s/list_node.json").expect("file should open read only");
            let json_transformed: serde_json::Value = serde_json::from_reader(file).expect("file should be proper JSON");
         
            let mut list_node: Vec<String> = Vec::new();
            // println!("{:?}", json_transformed);
            // println!("---------------------------");
            // println!("{:?}", json_transformed["items"]);
            // println!("---------------------------");
            if let Some(li_item) = json_transformed["items"].as_array(){
                for item in li_item {
                    if let Some(name) = item["metadata"]["name"].as_str() {
                        // Append the node name to the list
                        list_node.push(name.to_string());
                    } else {
                        println!("Node name is not a valid string.");
                    }
                }
                
            }
            return Ok(list_node);
            /// 
            /// 
            ////////////////////////
            return Err(err);
        }
    }
}

pub fn populate(token_k8s: String, base_url: String){
}


fn print_type_of<T>(_: &T) {
    println!("Type:{}\n", std::any::type_name::<T>())
}

#[tokio::main]
pub async fn gather_values(token_k8s: &String, base_url: &String, li_node: &mut Vec<String>, all_measures: &mut HashMap<String, Vec<String>>) {
    let formated_header = format!("Bearer {}",token_k8s);

    let timeout = Duration::new(5, 0);
    let client = ClientBuilder::new().timeout(timeout).build().expect("Error when trying to create a client");

    for node in li_node{
        let request_url = format!("{}/api/v1/nodes/{}/proxy/metrics/cadvisor", base_url, node);
        println!("--{}", request_url);
        // let response = client.get(&request_url)
        //                         .header("Authorization", &formated_header)
        //                         .send()
        //                         .await
        //                         .expect("failed to get response")
        //                         .text()
        //                         .await
        //                         .expect("failed to get payload");

        // Continuation of the main function
        let response = std::fs::read_to_string("/home/cyprien/code/alumet/plugin-k8s/cadvisor_raw_return.txt").expect("file should open read only");

        let response_as_lines = response.lines(); 
        print_type_of(&response_as_lines);
        for (i, line) in response_as_lines.enumerate() {
            if !line.starts_with("#"){
                // println!("Line:{}\n", line);
                print_type_of(&line);
                let prom: PrometheusMetric = PrometheusMetric::from_str(&line).expect(&format!("Prometheus test line #{i} failed to parse"));
                println!("Prometheus metric: {:?}", prom);
                if prom.name.eq("container_cpu_usage_seconds_total"){
                    todo!("Ajouter à la hashmap global")
                    // println!("{}", hash_map.contains_key("1"));
                }
                
        
            }
        }
    
    }
}