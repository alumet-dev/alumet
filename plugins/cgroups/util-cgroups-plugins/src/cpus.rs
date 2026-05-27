//! Gather information about CPU cores.
// (copied from the rapl plugin)
use std::num::ParseIntError;

use anyhow::Context;

fn parse_cpu_list(cpulist: &str) -> anyhow::Result<Vec<u32>> {
    // handles "n" or "start-end"
    fn parse_cpulist_item(item: &str) -> anyhow::Result<Vec<u32>> {
        let bounds: Vec<u32> = item
            .split('-')
            .map(str::parse)
            .collect::<Result<Vec<u32>, ParseIntError>>()?;

        match *bounds.as_slice() {
            [start, end] => Ok((start..=end).collect()),
            [n] => Ok(vec![n]),
            _ => Err(anyhow::anyhow!("invalid cpu_list: {}", item)),
        }
    }

    // this can be "0,64" or "0-1" or maybe "0-1,64-66"
    let cpus: Vec<u32> = cpulist
        .trim_end()
        .split(',')
        .map(parse_cpulist_item)
        .collect::<anyhow::Result<Vec<Vec<u32>>>>()?
        .into_iter() // not the same as iter() !
        .flatten()
        .collect();

    Ok(cpus)
}

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
    fn test_parse_cpu_list() {
        let cpulist = "1-2,3-4";
        let result = parse_cpu_list(cpulist).unwrap();
        assert_eq!(result, vec![1, 2, 3, 4]);
    }

    #[test]
    fn test_parse_cpulist_invalid() {
        let cpulist = "1-2-3";
        let result = parse_cpu_list(cpulist).unwrap_err();
        assert!(result.to_string().contains("invalid cpu_list"));
    }

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
