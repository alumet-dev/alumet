use std::time::Duration;
use alumet::{pipeline::trigger::Trigger, plugin::{rust::AlumetPlugin, ConfigTable}};

pub struct K8sPlugin{
    poll_interval: Duration,
}

mod cgroup_v2;
mod parsing_cgroupv2;
mod k8s_probe;

use crate::cgroup_v2::CgroupV2MetricFile;
use crate::k8s_probe::K8SProbe;

impl AlumetPlugin for K8sPlugin{
    fn name() -> &'static str {
        "K8S"
    }

    fn version() -> &'static str {
        "0.0.1"
    }

    fn init(_config: ConfigTable) -> anyhow::Result<Box<Self>> {
        // TODO read from config
        let poll_interval = Duration::from_secs(1);
        Ok(Box::new(K8sPlugin { poll_interval }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetStart) -> anyhow::Result<()> {
        let v2_used: bool = cgroup_v2::is_cgroups_v2();
        if v2_used == true{
            let final_li_metric_file: Vec<CgroupV2MetricFile> = cgroup_v2::list_all_k8s_pods_file();
            // I suppose value are 64 bit long
            let metrics_result = k8s_probe::Metrics::new(alumet);
            match metrics_result {
                Ok(metrics) => {
                    let probe = K8SProbe::new(metrics.clone(), final_li_metric_file)?;
                    alumet.add_source(Box::new(probe), Trigger::at_interval(self.poll_interval));  
                    println!("\n------------------------------------\n");     
                },
                Err(_) => todo!()
            }
            return Ok(());
        }else{
            panic!("Cgroups v2 are not being used!");
        }
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