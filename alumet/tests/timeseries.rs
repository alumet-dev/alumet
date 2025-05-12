// use alumet::{
//     measurement::MeasurementBuffer,
//     pipeline::{
//         elements::{error::TransformError, transform::TransformContext},
//         Transform,
//     },
//     timeseries::{self, TimeseriesProcessor},
// };

// #[test]
// fn test() {
//     struct Test {
//         tp: TimeseriesProcessor,
//     }

//     impl Transform {
//         pub fn new() -> Self {
//             Self {
//                 tp: timeseries::Builder::new()
//                     .group_by(&[Metric, Resource, Consumer, Attributes])
//                     .hopping_window(1)
//                     .build(),
//             }
//         }
//     }

//     impl Transform for Test {
//         fn apply(
//             &mut self,
//             measurements: &mut MeasurementBuffer,
//             ctx: &TransformContext,
//         ) -> Result<(), TransformError> {
//             let series = self.tp.process(measurements);
//             for (key, group) in series.iter() {
//                 // print to debug
//             }

//             let synchronized = series.synchronize_on(GroupKey::metric(rapl_consumed_energy), Interpolation::Triangular);
//             for (t, grouped_values) in synchronized {
//                 /*
//                  t0, {metric: rapl_consumed_energy, resource: CpuPkg(0), consumer: LocalMachine, ...}
//                  t0, {metric: rapl_consumed_energy, resource: CpuPkg(1), consumer: LocalMachine, ...}
//                  t0', {metric: procfs_cpu_usage, resource: CpuPkg(0), consumer: Process(123), ...}
//                  t0', {metric: procfs_cpu_usage, resource: CpuPkg(0), consumer: Process(456), ...}
//                  t0', {metric: procfs_cpu_usage, resource: CpuPkg(1), consumer: Process(123), ...}
//                  t0', {metric: procfs_cpu_usage, resource: CpuPkg(0), consumer: Process(456), ...}
//                  t1, ...

//                 group_by((metric, resource, consumer))
//                 =>
//                  group {metric: rapl_consumed_energy, resource: CpuPkg(0), consumer: LocalMachine} -> [(t0, ...), (t1, ...)]
//                  group {metric: rapl_consumed_energy, resource: CpuPkg(1), consumer: LocalMachine} -> [(t0, ...), (t1, ...)]
//                  group {metric: procfs_cpu_usage, resource: CpuPkg(0), consumer: Process(123)} -> [(t0', ...), (t1, ...)]
//                  group {metric: procfs_cpu_usage, resource: CpuPkg(0), consumer: Process(456)} -> [(t0', ...), (t1, ...)]
//                  group {metric: procfs_cpu_usage, resource: CpuPkg(1), consumer: Process(123)} -> [(t0', ...), (t1, ...)]
//                  group {metric: procfs_cpu_usage, resource: CpuPkg(1), consumer: Process(456)} -> [(t0', ...), (t1, ...)]

//                 synchronize(
//                     master=group {metric: rapl_consumed_energy, resource: CpuPkg(0), consumer: LocalMachine},
//                     ignore_diff=group {metric: rapl_consumed_energy, resource: other*, consumer: LocalMachine},
//                     interpolate=others*
//                 )
//                 =>
//                  t0 -> [
//                     (group {metric: rapl_consumed_energy, resource: CpuPkg(0), consumer: LocalMachine}, ...),
//                     (group {metric: rapl_consumed_energy, resource: CpuPkg(1), consumer: LocalMachine}, ...),
//                     (group {metric: procfs_cpu_usage, resource: CpuPkg(0), consumer: Process(123)}, ...),
//                     (group {metric: procfs_cpu_usage, resource: CpuPkg(0), consumer: Process(456)}, ...),
//                     (group {metric: procfs_cpu_usage, resource: CpuPkg(1), consumer: Process(123)}, ...),
//                     (group {metric: procfs_cpu_usage, resource: CpuPkg(1), consumer: Process(456)}, ...),
//                  ],
//                  t1 -> [...]

//                 for each timestamp {
//                     for each group {
//                         if let Process(_) = group.consumer {
//                             pkg_energy = timestamp[group{metric: rapl_consumed_energy, resource: group.resource, consumer: LocalMachine}]
//                             pkg_usage = timestamp[group{metric: procfs_cpu_usage, resource: group.resource, consumer: group.consumer}]
//                             attributed_energy_for_process_on_pkg = pkg_energy * pkg_usage / dt
//                             point = new_point(timestamp, ..., value=attributed_energy_for_process_on_pkg)
//                             push(point) at timestamp
//                         }
//                     }
//                 }
//                 =>
//                  t0 -> [
//                     ...,
//                     ({value: <attributed_energy_for_process_on_pkg> })
//                  ],
//                  t1 -> [
//                     ...,
//                     ({value: <attributed_energy_for_process_on_pkg> })
//                  ]
//                 */
//                 let value = grouped_values[(rapl_consumed_energy, ...)] * grouped_values[(cpu_usage, ...)];
//                 grouped_values.push(metric, auto_resource, auto_consumer, value);
//             }
//             // for (t, values) in synchronized {
//             //     // t: Timestamp
//             //     // values: SynchronizedValues (one per group)
//             //     values.add(attributed_pod_energy) = values[rapl_consumed_energy] * values[pod_cpu_usage];
//             // }
//             Ok(())
//         }
//     }
// }
