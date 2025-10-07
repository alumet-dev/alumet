use alumet::measurement::AttributeValue;
use util_cgroups::Cgroup;
use util_cgroups_plugins::regex::RegexAttributesExtrator;

pub const JOB_REGEX_SLURM1: &str = "/slurm/uid_(?<user_id__u64>[0-9]+)/job_(?<job_id__u64>[0-9]+)";
pub const JOB_REGEX_SLURM2: &str = "/slurmstepd.scope/job_(?<job_id__u64>[0-9]+)";
pub const JOB_STEP_REGEX: &str = "step_((?<job_step>[0-9a-zA-Z]+)).*";

pub fn find_jobid_in_attrs(attrs: &Vec<(String, AttributeValue)>) -> Option<u64> {
    attrs.iter().find(|(k, _)| k == "job_id").map(|(_, v)| match v {
        AttributeValue::U64(id) => *id,
        _ => unreachable!("job_id should be a u64, is the regex correct?"),
    })
}

#[derive(Clone)]
pub struct JobTagger {
    extractor_v1: RegexAttributesExtrator,
    extractor_v2: RegexAttributesExtrator,
    step_extractor: RegexAttributesExtrator,
}

impl JobTagger {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            extractor_v1: RegexAttributesExtrator::new(JOB_REGEX_SLURM1)?,
            extractor_v2: RegexAttributesExtrator::new(JOB_REGEX_SLURM2)?,
            step_extractor: RegexAttributesExtrator::new(JOB_STEP_REGEX)?,
        })
    }

    pub fn attributes_for_cgroup(&self, cgroup: &Cgroup) -> Vec<(String, AttributeValue)> {
        // extracts attributes "job_id" and ("user" or "user_id")
        let extractor = match cgroup.hierarchy().version() {
            util_cgroups::CgroupVersion::V1 => &self.extractor_v1,
            util_cgroups::CgroupVersion::V2 => &self.extractor_v2,
        };

        let mut attrs = extractor
            .extract(cgroup.canonical_path())
            .expect("bad regex: it should only match if the input can be parsed into the specified types");

        let is_job = !attrs.is_empty();

        if is_job {
            // check if the cgroup is a job step and extract its name as a "job_step" attribute
            self.step_extractor
                .extract_into(cgroup.canonical_path(), &mut attrs)
                .expect("bad regex: it should only match if the input can be parsed into the specified types");
        }
        attrs
    }
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
