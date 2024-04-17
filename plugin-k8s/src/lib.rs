use std::collections::HashMap;

use anyhow::{Error, anyhow};

pub struct K8sPlugin;

mod cgroup_v2;

impl alumet::plugin::Plugin for K8sPlugin{
    fn name(&self) -> &str {
        "K8S"
    }

    fn version(&self) -> &str {
        "0.0.1"
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetStart) -> anyhow::Result<()> {
        let v2_used: bool = cgroup_v2::is_cgroups_v2();
        println!("The result is: {:?}", v2_used);
        if v2_used == true{
            println!("Cgroups v2 are being used.");
            cgroup_v2::list_all_K8S_pods();
        }else{
            panic!("Cgroups v2 are not being used!");
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