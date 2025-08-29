use alumet::measurement::AttributeValue;

pub const JOB_REGEX_SLURM1: &str = "/slurm/uid_(?<user_id__u64>[0-9]+)/job_(?<job_id__u64>[0-9]+)";
pub const JOB_REGEX_SLURM2: &str = "/slurmstepd.scope/job_(?<job_id__u64>[0-9]+)";
pub const JOB_STEP: &str = "step_((?<job_step>[0-9a-zA-Z]+)).*";

pub fn find_jobid_in_attrs(attrs: &Vec<(String, AttributeValue)>) -> Option<u64> {
    attrs.iter().find(|(k, _)| k == "job_id").map(|(_, v)| match v {
        AttributeValue::U64(id) => *id,
        _ => unreachable!("job_id should be a u64, is the regex correct?"),
    })
}

#[cfg(test)]
mod tests {
    use crate::attr::*;
    use alumet::measurement::AttributeValue;

    #[test]
    fn test_find_jobid_in_attrs() {
        let attrs: Vec<(String, AttributeValue)> = vec![
            ("job_id".to_string(), AttributeValue::U64(19)),
            ("Saphira".to_string(), AttributeValue::String("Eragon".to_string())),
        ];
        assert_eq!(find_jobid_in_attrs(&attrs), Some(19));
    }

    #[test]
    fn test_find_jobid_in_attrs_not_existing() {
        let attrs: Vec<(String, AttributeValue)> = vec![
            ("not_job_id".to_string(), AttributeValue::U64(15)),
            ("Glaedr".to_string(), AttributeValue::String("Oromis".to_string())),
        ];
        assert_eq!(find_jobid_in_attrs(&attrs), None);
    }

    #[test]
    fn test_find_jobid_in_empty_vec() {
        let attrs: Vec<(String, AttributeValue)> = vec![];
        assert_eq!(find_jobid_in_attrs(&attrs), None);
    }
}
