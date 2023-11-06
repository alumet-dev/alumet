use std::error::Error;

use locomen_core::metrics::{MetricId, MetricBuffer, MetricSource, MetricTransformer, Metric, TypedMetric, Metric2};


#[test]
fn test_input_api() {
    println!("size_of u64 {}", std::mem::size_of::<u64>());
    println!("size_of MetricId {}", std::mem::size_of::<MetricId<u64>>());
    println!("size_of MetricId {}", std::mem::size_of::<MetricId<f64>>());
    println!("size_of TypedMetric {}", std::mem::size_of::<TypedMetric>());
    println!("size_of Metric {}", std::mem::size_of::<Metric>());
    println!("size_of Metric2 {}", std::mem::size_of::<Metric2>());
    println!("size_of Vec<> {}", std::mem::size_of::<Vec<(String,String)>>());
}

struct TestPlugin {}

// impl LocomenPlugin for TestPlugin {
//     fn init(&mut self, locomen: &mut Locomen) -> Result<()> {
//         let rapl_energy_metric = locomen.new_metric::<u64>("rapl_energy");
//         let pid_cpu_usage_metric = locomen.new_metric::<f64>("cpu_usage");
//         let input = TestInput {
//             rapl_energy_metric,
//             pid_cpu_usage_metric,
//             processes: vec![],
//             disks: vec![],
//         };
//         Ok(())
//     }
// }

struct TestInput {
    rapl_energy_metric: MetricId<u64>,
    pid_cpu_usage_metric: MetricId<f64>,
    disk_usage_metric: MetricId<u64>,
    processes: Vec<u64>,
    disks: Vec<String>,
}

impl MetricSource for TestInput {
    type Err = Box<dyn Error>;

    fn poll(&self, buf: &mut MetricBuffer) -> Result<(), Self::Err> {
        // get RAPL energy value
        let core_energy = 10u64;
        let pkg_energy = 50u64;
        buf.add(&self.rapl_energy_metric, core_energy, vec![("domain", "core")]);
        buf.add(&self.rapl_energy_metric, pkg_energy, vec![("domain", "pkg")]);

        // get process cpu stat
        for p in &self.processes {
            let cpu_usage = 1.25;
            buf.add(&self.pid_cpu_usage_metric, cpu_usage, vec![("pid", &p.to_string())]);
        }

        // get disk stat
        for d in &self.disks {
            let disk_used_mo = 10_420;
            let mount_path = format!("/mnt/disk_{d}");
            buf.add(&self.disk_usage_metric, disk_used_mo, vec![("mount", &mount_path), ("uid", d)]);
        }
        Ok(())
    }
}

struct TestTranformer {}

impl MetricTransformer for TestTranformer {
    type Err = Box<dyn Error>;

    fn transform(&self, m: &mut MetricBuffer) -> Result<(), Self::Err> {
        let test_metric_u64 = Metric { typed: TypedMetric::U64 { id: todo!(), value: 50 }, metadata: vec![("domain".to_owned(), "core".to_owned())] };
        let test_metric_f64 = Metric { typed: TypedMetric::F64 { id: todo!(), value: 1.25 }, metadata: vec![("pid".to_owned(), "1234".to_owned())] };
        let mut metrics = vec![test_metric_u64, test_metric_f64];
        metrics.iter()
        .for_each(|metric| {
            // if let Some(pid) = metric.metadata.get("pid") {
            //     let container = containers.get_for_pid(pid);
            //     metric.add_metadata("container", container);
            // }
        });
        Ok(())
    }
}
