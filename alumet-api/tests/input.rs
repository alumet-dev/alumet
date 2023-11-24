// use std::{collections::HashMap, error::Error};

// #[test]
// fn test_input_api() {
//     // With a dynamically loaded plugin, this struct will be filled by Locomen CLI app.
//     // The plugin's binary must contain the symbols PLUGIN_NAME, PLUGIN_VERSION and plugin_init.
//     let manual_plugin = LocomenPlugin {
//         name: "test_input",
//         version: "0.0.1",
//         init: crate::init,
//     };
//     let locomen = Locomen::new();
//     let test_input_plugin: TestInputPlugin = locomen.plugins().register(manual_plugin)?;
// }

// struct TestInputPlugin {
//     cpu_ids: Vec<u32>,
//     cpu_consumption: TypedMetricId<u64, CpuConsumptionAttributes>,
//     disk_usage: MetricId<f64, ()>,
// }

// //#[derive(Attributes)]
// struct CpuConsumptionAttributes {
//     cpu_id: u32,
// }

// pub fn init(&mut locomen: PluginInitializer) -> Result<TestInputPlugin, Box<dyn Error>> {
//     let cpu_consumption = locomen
//         .new_metric("cpu_consumption", MetricType::U64, MetricUnit::Joules)
//         .description(
//             "Energy consumption of the whole CPU socket, since the previous measurement",
//         )
//         .build()?;

//     let disk_usage = locomen
//         .new_metric("disk_usage", MetricType::F64, MetricUnit::Percent)
//         .description("Percentage of allocated disk space")
//         .build()?;

//     let cpu_ids = vec![0]; // monitor cpu 0
//     let f = 1.0; // 1 Hz

//     locomen
//         .inputs()
//         .register_polling(self, PollingPolicy::Frequency(f));
    
//     Ok(TestInputPlugin { cpu_ids, cpu_consumption, disk_usage })
// }

// impl TestInputPlugin {
//     pub fn poll(&mut self, &mut dest: MetricAccumulator) {
//         fn measure_consumption(cpu_id: u32) -> u64 {
//             return 123; // dummy measurement
//         }
//         fn measure_usage() -> f64 {
//             return 0.5;
//         }
//         dest.push(self.disk_usage, measure_usage(), ());

//         for cpu_id in self.cpu_ids {
//             let measurement = measure_consumption(cpu_id);
//             dest.push_detailed(
//                 self.cpu_consumption,
//                 measurement,
//                 CpuConsumptionAttributes { cpu_id },
//             );
//             // or
//             dest.push_detailed(
//                 self.cpu_consumption,
//                 measurement,
//                 vec![("cpu_id", cpu_id)]
//             )
//         }
//     }
// }
