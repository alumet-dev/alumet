use std::{str::FromStr, string::String};

#[derive(Debug, PartialEq, Clone)]
pub struct CgroupV2Metric {
    pub name: String,
    pub time_used_tot: u64,
    pub time_used_user_mode: u64,
    pub time_used_system_mode: u64,
}

impl FromStr for CgroupV2Metric {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut cgroup_struc_to_ret = CgroupV2Metric {
            name: "".to_owned(),
            time_used_tot: 0,
            time_used_user_mode: 0,
            time_used_system_mode: 0,
        };
        for (_i, line) in s.lines().enumerate() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                match parts[0] {
                    "usage_usec" => {
                        cgroup_struc_to_ret.time_used_tot = parts[1].parse::<u64>()?;
                    }
                    "user_usec" => {
                        cgroup_struc_to_ret.time_used_user_mode = parts[1].parse::<u64>()?;
                    }
                    "system_usec" => {
                        cgroup_struc_to_ret.time_used_system_mode = parts[1].parse::<u64>()?;
                    }
                    &_ => continue,
                }
            }
        }
        return Ok(cgroup_struc_to_ret);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parser() {
        let str1 = format!(
            "usage_usec 1111\n
        user_usec 222222222222222222\n
        system_usec 33\n
        nr_periods 0\n
        nr_throttled 0\n
        throttled_usec 0"
        );
        let result: CgroupV2Metric = CgroupV2Metric::from_str(&str1).unwrap();
        assert_eq!(result.name, "");
        assert_eq!(result.time_used_tot, 1111 as u64);
        assert_eq!(result.time_used_user_mode, 222222222222222222 as u64);
        assert_eq!(result.time_used_system_mode, 33 as u64);

        let str2 = format!(
            "nr_throttled 0\n
        usage_usec 1111\n
        system_usec 33"
        );
        let result: CgroupV2Metric = CgroupV2Metric::from_str(&str2).unwrap();
        assert_eq!(result.name, "");
        assert_eq!(result.time_used_tot, 1111 as u64);
        assert_eq!(result.time_used_user_mode, 0 as u64);
        assert_eq!(result.time_used_system_mode, 33 as u64);

        let str3 = format!(
            "
        system_usec 33"
        );
        let result: CgroupV2Metric = CgroupV2Metric::from_str(&str3).unwrap();
        assert_eq!(result.name, "");
        assert_eq!(result.time_used_tot, 0 as u64);
        assert_eq!(result.time_used_user_mode, 0 as u64);
        assert_eq!(result.time_used_system_mode, 33 as u64);

        let str4 = format!(
            "usage_usec 1111\n
        system_usec 33\n
        user_usec 222222222222222222\n
        throttled_usec 0"
        );
        let result: CgroupV2Metric = CgroupV2Metric::from_str(&str4).unwrap();
        assert_eq!(result.name, "");
        assert_eq!(result.time_used_tot, 1111 as u64);
        assert_eq!(result.time_used_user_mode, 222222222222222222 as u64);
        assert_eq!(result.time_used_system_mode, 33 as u64);

        let str5 = format!("");
        let result: CgroupV2Metric = CgroupV2Metric::from_str(&str5).unwrap();
        assert_eq!(result.name, "");
        assert_eq!(result.time_used_tot, 0 as u64);
        assert_eq!(result.time_used_user_mode, 0 as u64);
        assert_eq!(result.time_used_system_mode, 0 as u64);

        let str6 = format!(
            "nr_periods 0\n
        nr_throttled 0\n
        throttled_usec 0"
        );
        let result: CgroupV2Metric = CgroupV2Metric::from_str(&str6).unwrap();
        assert_eq!(result.name, "");
        assert_eq!(result.time_used_tot, 0 as u64);
        assert_eq!(result.time_used_user_mode, 0 as u64);
        assert_eq!(result.time_used_system_mode, 0 as u64);
    }
}
