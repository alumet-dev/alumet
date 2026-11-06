//! Gather information about CPU cores.
// (copied from the rapl plugin)
use std::num::ParseIntError;

use anyhow::Context;
use util_cgroups::measure::v2::cpu::parse_cpu_list;

fn online_cpus_path(path: &std::path::Path) -> anyhow::Result<Vec<u32>> {
    let list = std::fs::read_to_string(path).with_context(|| format!("failed to parse {}", path.display()))?;
    parse_cpu_list(&list)
}

pub fn online_cpus() -> anyhow::Result<Vec<u32>> {
    online_cpus_path(std::path::Path::new("/sys/devices/system/cpu/online"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_online_cpus_path() -> anyhow::Result<()> {
        let mut file = tempfile::NamedTempFile::new()?;
        let cpulist = "1-3,5-6";
        writeln!(file, "{cpulist}")?;

        let result = online_cpus_path(file.path())?;
        assert_eq!(result, vec![1, 2, 3, 5, 6]);

        Ok(())
    }
}
