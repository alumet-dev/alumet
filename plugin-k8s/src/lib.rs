use std::collections::HashMap;
use std::time::Duration;
use alumet::{pipeline::{trigger::Trigger, Source}, plugin::{rust::AlumetPlugin, ConfigTable}, units::Unit};

use anyhow::{Error, anyhow};

pub struct K8sPlugin{
    poll_interval: Duration,
}

mod cgroup_v2;
mod parsing_cgroupv2;
mod k8s_probe;

use crate::{parsing_cgroupv2::CgroupV2Metric};
use crate::cgroup_v2::CgroupV2MetricFile;
use crate::k8s_probe::{K8SProbe, Metrics};

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
        let mut all_value_saved: HashMap<String, CgroupV2Metric> = HashMap::new();

        let v2_used: bool = cgroup_v2::is_cgroups_v2();
        println!("The result is: {:?}", v2_used);
        if v2_used == true{
            println!("Cgroups v2 are being used.");
            let final_li_metric_file: Vec<CgroupV2MetricFile> = cgroup_v2::list_all_K8S_pods_file();
            println!("Found: {} files to read", final_li_metric_file.len());
            for elem in &final_li_metric_file{
                println!("{:?}", elem);
            }
            println!("-----------------------------------");
            let first_metrics: Vec<CgroupV2Metric> = cgroup_v2::gather_value(&final_li_metric_file);

            // I suppose value are 64 bit long
            
            let metrics_result = k8s_probe::Metrics::new(alumet);
            // let metric_tot = alumet.create_metric::<u64>("total_usage_usec", usec.clone(), "Total CPU usage time by the group")?;
            // let metric_usr = alumet.create_metric::<u64>("user_usage_usec", usec.clone(), "User CPU usage time by the group")?;
            // let metric_sys = alumet.create_metric::<u64>("system_usage_usec", usec.clone(), "System CPU usage time by the group")?;
            // let metric = CgroupV2Metric {
            //     time_used_tot: metric_tot,
            //     time_used_user_mode: metric_usr,
            //     time_used_system_mode: metric_sys,
            // };
            // let mut events_on_cpus: Vec<CgroupV2Metric> = Vec::new();
            // for event in &available_domains.perf_events {
            //     for cpu in &socket_cpus {
            //         events_on_cpus.push((event, cpu));
            //     }
            // }
            // log::debug!("Events to read: {events_on_cpus:?}");
            match metrics_result {
                Ok(metrics) => {
                    for pod in first_metrics {
                        let probe = K8SProbe::new(metrics.clone(), pod.name)?;
                        alumet.add_source(Box::new(probe), Trigger::at_interval(self.poll_interval));  
                        println!("\n------------------------------------\n");  
                    }
                },
                Err(_) => todo!()
            }
            
            
            return Ok(());
    


            // for elem in &first_metrics{
            //     println!("{:?}", elem);
            // }
            // cgroup_v2::diff_value(&mut all_value_saved, first_metrics);
            // for i in 1..10{
            //     let tmpVec: Vec<CgroupV2Metric> = cgroup_v2::gather_value(&final_li_metric_file);
            //     cgroup_v2::diff_value(&mut all_value_saved, tmpVec);
            //     println!("\n------------------------------------\n");
            // }

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