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

pub fn online_cpus() -> anyhow::Result<Vec<u32>> {
    let path = "/sys/devices/system/cpu/online";
    let list = std::fs::read_to_string(path).with_context(|| format!("failed to parse {path}"))?;
    parse_cpu_list(&list)
}
