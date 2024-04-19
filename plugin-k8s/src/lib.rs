use std::collections::HashMap;
use alumet::units::Unit;

use anyhow::{Error, anyhow};

pub struct K8sPlugin;

mod cgroup_v2;
mod parsing_cgroupv2;
mod k8s_probe;

use crate::parsing_cgroupv2::CgroupV2Metric;
use crate::cgroup_v2::CgroupV2MetricFile;
use crate::k8s_probe::K8SProbe;

impl alumet::plugin::Plugin for K8sPlugin{
    fn name(&self) -> &str {
        "K8S"
    }

    fn version(&self) -> &str {
        "0.0.1"
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
            let usec = Unit::CustomUnit{
                unique_name: "usec",
                display_name: "µsec",
                debug_name: "µsec",
            };
            let metric = alumet.create_metric::<u64>("total_usage_usec", Unit::Custom(usec), "Total CPU usage time by the group")?;
            let metric = alumet.create_metric::<u64>("user_usage_usec", Unit::Custom(usec), "User CPU usage time by the group")?;
            let metric = alumet.create_metric::<u64>("system_usage_usec", Unit::Custom(usec), "System CPU usage time by the group")?;
            let mut events_on_cpus = Vec::new();
            for event in &available_domains.perf_events {
                for cpu in &socket_cpus {
                    events_on_cpus.push((event, cpu));
                }
            }
            log::debug!("Events to read: {events_on_cpus:?}");
            let probe = PerfEventProbe::new(metric, &events_on_cpus)?;
            alumet.add_source(Box::new(probe));
            Ok(())
    


            // for elem in &first_metrics{
            //     println!("{:?}", elem);
            // }
            cgroup_v2::diff_value(&mut all_value_saved, first_metrics);
            for i in 1..10{
                let tmpVec: Vec<CgroupV2Metric> = cgroup_v2::gather_value(&final_li_metric_file);
                cgroup_v2::diff_value(&mut all_value_saved, tmpVec);
                println!("\n------------------------------------\n");
            }

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