use anyhow::anyhow;

pub struct K8sPlugin;

mod request_cadvisor;

impl alumet::plugin::Plugin for K8sPlugin{
    fn name(&self) -> &str {
        "K8S"
    }

    fn version(&self) -> &str {
        "0.0.1"
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetStart) -> anyhow::Result<()> {
        let token_k8s = request_cadvisor::retrieve_token();
        let apiServer: String = String::from("https://10.22.80.14:6443");
        let li_node = request_cadvisor::get_all_kubernetes_nodes(token_k8s, apiServer);
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
