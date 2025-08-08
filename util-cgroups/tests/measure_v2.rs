use std::{fs::File, io::Write};

use tempfile::tempdir;
use util_cgroups::{
    measure::v2::{cpu::CpuStatCollectorSettings, memory::MemoryStatCollectorSettings, V2Collector},
    Cgroup, CgroupHierarchy, CgroupVersion,
};

#[test]
pub fn test_new_and_measure() -> anyhow::Result<()> {
    let root = tempdir().expect("Failed to create a temporary directory");
    // file 1
    let file_path = root.path().join("cpu.stat");
    let data_cpu = "usage_usec 579\n\
            user_usec 123\n\
            system_usec 456\n\
            nr_periods 1\n\
            nr_throttled 2\n\
            throttled_usec 3\n\
            nr_bursts 4\n\
            burst_usec 5\n";
    let mut file1 = File::create(file_path)?;
    file1.write_all(data_cpu.as_bytes())?;
    // file 2
    let file_path = root.path().join("memory.stat");
    let data_mem = "anon 321\n\
            file 654\n\
            kernel_stack 987\n\
            pagetables 741\n";
    let mut file2 = File::create(file_path)?;
    file2.write_all(data_mem.as_bytes())?;
    // file 3
    let file_path = root.path().join("memory.current");
    let data_mem_cur = "852";
    let mut file3 = File::create(file_path)?;
    file3.write_all(data_mem_cur.as_bytes())?;

    let hierarchy = CgroupHierarchy::manually_unchecked(root.path(), CgroupVersion::V2, vec!["cpu", "memory"]);
    let cgroup = Cgroup::from_fs_path(&hierarchy, root.path().to_path_buf());

    let mut io_buf = Vec::new();
    let mut collector = V2Collector::new(
        cgroup,
        MemoryStatCollectorSettings::default(),
        CpuStatCollectorSettings::default(),
        &mut io_buf,
    )?;
    let v2stat_res = collector.measure(&mut io_buf);
    assert!(v2stat_res.is_ok());
    let v2stat = v2stat_res.unwrap();
    assert!(v2stat.cpu_stat.is_some());
    assert!(v2stat.memory_stat.is_some());
    assert!(v2stat.memory_current.is_some());
    let cpu_stat = v2stat.cpu_stat.unwrap();
    let mem_stat = v2stat.memory_stat.unwrap();
    let mem_cur = v2stat.memory_current.unwrap();

    assert_eq!(cpu_stat.system.unwrap_or(0), 456);
    assert_eq!(cpu_stat.user.unwrap_or(0), 123);
    assert_eq!(cpu_stat.usage.unwrap_or(0), 579);

    assert_eq!(mem_stat.anon.unwrap_or(0), 321);
    assert_eq!(mem_stat.file.unwrap_or(0), 654);
    assert_eq!(mem_stat.kernel_stack.unwrap_or(0), 987);
    assert_eq!(mem_stat.page_tables.unwrap_or(0), 741);

    assert_eq!(mem_cur, 852);

    Ok(())
}

#[test]
pub fn test_new_and_measure_bad_file_content_key() -> anyhow::Result<()> {
    let root = tempdir().expect("Failed to create a temporary directory");
    // file 1
    let file_path = root.path().join("cpu.stat");
    let data_cpu = "usage 579\n\
            user 123\n\
            system 456\n\
            nr_pe 1\n\
            nr_thrtled 2\n\
            throttd_usec 3\n\
            nr_buts 4\n\
            burstc 5\n";
    let mut file1 = File::create(file_path)?;
    file1.write_all(data_cpu.as_bytes())?;
    // file 2
    let file_path = root.path().join("memory.current");
    let data_mem_cur = "963";
    let mut file3 = File::create(file_path)?;
    file3.write_all(data_mem_cur.as_bytes())?;

    let hierarchy = CgroupHierarchy::manually_unchecked(root.path(), CgroupVersion::V2, vec!["cpu", "memory"]);
    let cgroup = Cgroup::from_fs_path(&hierarchy, root.path().to_path_buf());

    let mut io_buf = Vec::new();
    let mut collector = V2Collector::new(
        cgroup,
        MemoryStatCollectorSettings::default(),
        CpuStatCollectorSettings::default(),
        &mut io_buf,
    )?;
    let v2stat_res = collector.measure(&mut io_buf);
    assert!(v2stat_res.is_ok());
    let v2stat = v2stat_res.unwrap();
    assert!(v2stat.cpu_stat.is_some());
    assert!(v2stat.memory_stat.is_none());
    assert!(v2stat.memory_current.is_some());
    let cpu_stat = v2stat.cpu_stat.unwrap();
    let mem_cur = v2stat.memory_current.unwrap();

    assert!(cpu_stat.system.is_none());
    assert!(cpu_stat.usage.is_none());
    assert!(cpu_stat.user.is_none());

    assert_eq!(mem_cur, 963);
    Ok(())
}

#[test]
pub fn test_new_files_dont_exist() -> anyhow::Result<()> {
    let root = tempdir().expect("Failed to create a temporary directory");

    let hierarchy = CgroupHierarchy::manually_unchecked(root.path(), CgroupVersion::V2, vec!["cpu", "memory"]);
    let cgroup = Cgroup::from_fs_path(&hierarchy, root.path().to_path_buf());

    let mut io_buf = Vec::new();
    let mut collector = V2Collector::new(
        cgroup,
        MemoryStatCollectorSettings::default(),
        CpuStatCollectorSettings::default(),
        &mut io_buf,
    )?;

    let v2stat_res = collector.measure(&mut io_buf);

    assert!(v2stat_res.is_ok());
    let v2stat = v2stat_res.unwrap();
    assert!(v2stat.cpu_stat.is_none());
    assert!(v2stat.memory_stat.is_none());
    assert!(v2stat.memory_current.is_none());
    Ok(())
}

#[test]
pub fn test_new_missing_lines() -> anyhow::Result<()> {
    let root = tempdir().expect("Failed to create a temporary directory");
    // file 1
    let file_path = root.path().join("cpu.stat");
    let data_cpu = "usage_usec 579\n\
            burst_usec 5\n";
    let mut file1 = File::create(file_path)?;
    file1.write_all(data_cpu.as_bytes())?;
    // file 2
    let file_path = root.path().join("memory.stat");
    let data_mem = "kernel_stack 987\n\
            pagetables 741\n";
    let mut file2 = File::create(file_path)?;
    file2.write_all(data_mem.as_bytes())?;
    // file 3
    let file_path = root.path().join("memory.current");
    let data_mem_cur = "159";
    let mut file3 = File::create(file_path)?;
    file3.write_all(data_mem_cur.as_bytes())?;

    let hierarchy = CgroupHierarchy::manually_unchecked(root.path(), CgroupVersion::V2, vec!["cpu", "memory"]);
    let cgroup = Cgroup::from_fs_path(&hierarchy, root.path().to_path_buf());

    let mut io_buf = Vec::new();
    let mut collector = V2Collector::new(
        cgroup,
        MemoryStatCollectorSettings::default(),
        CpuStatCollectorSettings::default(),
        &mut io_buf,
    )?;
    let v2stat_res = collector.measure(&mut io_buf);
    assert!(v2stat_res.is_ok());
    let v2stat = v2stat_res.unwrap();
    assert!(v2stat.cpu_stat.is_some());
    assert!(v2stat.memory_stat.is_some());
    assert!(v2stat.memory_current.is_some());
    let cpu_stat = v2stat.cpu_stat.unwrap();
    let mem_stat = v2stat.memory_stat.unwrap();
    let mem_cur = v2stat.memory_current.unwrap();

    assert!(cpu_stat.system.is_none());
    assert!(cpu_stat.user.is_none());
    assert_eq!(cpu_stat.usage.unwrap_or(0), 579);

    assert!(mem_stat.anon.is_none());
    assert!(mem_stat.file.is_none());
    assert_eq!(mem_stat.kernel_stack.unwrap_or(0), 987);
    assert_eq!(mem_stat.page_tables.unwrap_or(0), 741);

    assert_eq!(mem_cur, 159);

    Ok(())
}

#[test]
pub fn test_new_and_measure_bad_file_content_value() -> anyhow::Result<()> {
    let root = tempdir().expect("Failed to create a temporary directory");
    // file 1
    let file_path = root.path().join("cpu.stat");
    let data_cpu = "usage_usec azer\n\
            user_usec tyui\n\
            system_usec opfg\n\
            nr_periods 1\n\
            nr_throttled 2\n\
            throttled_usec 3\n\
            nr_bursts 4\n\
            burst_usec 5\n";
    let mut file1 = File::create(file_path)?;
    file1.write_all(data_cpu.as_bytes())?;
    // file 2
    let file_path = root.path().join("memory.stat");
    let data_mem = "anon ghj\n\
            file hh\n\
            kernel_stack rez\n\
            pagetables 741\n";
    let mut file2 = File::create(file_path)?;
    file2.write_all(data_mem.as_bytes())?;
    // file 3
    let file_path = root.path().join("memory.current");
    let data_mem_cur = "fghj";
    let mut file3 = File::create(file_path)?;
    file3.write_all(data_mem_cur.as_bytes())?;

    let hierarchy = CgroupHierarchy::manually_unchecked(root.path(), CgroupVersion::V2, vec!["cpu", "memory"]);
    let cgroup = Cgroup::from_fs_path(&hierarchy, root.path().to_path_buf());

    let mut io_buf = Vec::new();
    let collector_res = V2Collector::new(
        cgroup,
        MemoryStatCollectorSettings::default(),
        CpuStatCollectorSettings::default(),
        &mut io_buf,
    );

    assert!(collector_res.is_err());
    Ok(())
}
