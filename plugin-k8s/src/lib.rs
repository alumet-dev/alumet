use anyhow::{Error, anyhow};

pub struct K8sPlugin;

mod request_cadvisor;
mod parsing_prometheus;

impl alumet::plugin::Plugin for K8sPlugin{
    fn name(&self) -> &str {
        "K8S"
    }

    fn version(&self) -> &str {
        "0.0.1"
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetStart) -> anyhow::Result<()> {
        let token_k8s = request_cadvisor::retrieve_token();
        let api_server: String = String::from("https://10.22.80.14:6443");
        let mut li_node = request_cadvisor::get_all_kubernetes_nodes(&token_k8s, &api_server);
        match li_node {
            Ok(mut vecteur_node) => {
                for element in &mut vecteur_node {
                    println!("Borrowed: {}", element);
                }
                request_cadvisor::gather_values(&token_k8s, &api_server, &mut vecteur_node);
            },
            Err(err) => {
                println!("Error with return of get_all_kubernetes_nodes");
                return Err(anyhow!(err));
            }
        
        }

        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[test]
//     fn it_works() {
//         let result = add(2, 2);
//         assert_eq!(result, 4);
//     }
// }
