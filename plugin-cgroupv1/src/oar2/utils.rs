use alumet::resources::ResourceConsumer;
use std::fs::File;

#[derive(Debug)]
pub struct Cgroupv1MetricFile {
    /// Job ID of the pod.
    pub job_id: String,
    /// Path to the cgroup cpu stat file.
    pub cpu_file_path: ResourceConsumer,
    /// Path to the cgroup memory stat file.
    pub memory_file_path: ResourceConsumer,
    /// Opened file descriptor for cgroup cpu stat.
    pub cgroup_cpu_file: File,
    /// Opened file descriptor for cgroup memory stat.
    pub cgroup_memory_file: File,
}

impl Cgroupv1MetricFile {
    /// Create a new Cgroupv1MetricFile structure from a name, a path and a File.
    pub fn new(
        name: String,
        consumer_cpu: ResourceConsumer,
        consumer_memory: ResourceConsumer,
        file_cpu: File,
        file_memory: File,
    ) -> Cgroupv1MetricFile {
        Cgroupv1MetricFile {
            job_id: name,
            cpu_file_path: consumer_cpu,
            memory_file_path: consumer_memory,
            cgroup_cpu_file: file_cpu,
            cgroup_memory_file: file_memory,
        }
    }
}
