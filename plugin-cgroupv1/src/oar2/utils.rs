use alumet::resources::ResourceConsumer;
use std::fs::File;

#[derive(Debug)]
pub struct OpenedCgroupv1 {
    /// Job ID.
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
