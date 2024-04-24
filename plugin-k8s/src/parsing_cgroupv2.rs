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
        let mut cgroup_struc_to_ret = CgroupV2Metric{
                                            name: "".to_owned(),
                                            time_used_tot: 0,
                                            time_used_user_mode: 0,
                                            time_used_system_mode: 0,
                                        };
        for (_i, line) in s.lines().enumerate(){
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                match parts[0]{
                    "usage_usec" => {
                        if let Ok(tmp_value_parsed) = parts[1].parse::<u64>(){
                            cgroup_struc_to_ret.time_used_tot = tmp_value_parsed;
                        }
                    },
                    "user_usec" => {
                        if let Ok(tmp_value_parsed) = parts[1].parse::<u64>(){
                            cgroup_struc_to_ret.time_used_user_mode = tmp_value_parsed;
                        }
                    },
                    "system_usec" => {
                        if let Ok(tmp_value_parsed) = parts[1].parse::<u64>(){
                            cgroup_struc_to_ret.time_used_system_mode = tmp_value_parsed;
                        }
                    },
                    &_ => { continue
                    }
                }
            
            }
        }
        return Ok(cgroup_struc_to_ret);
    }
}
