use alumet::measurement::AttributeValue;
use util_cgroups::Cgroup;
use util_cgroups_plugins::regex::RegexAttributesExtrator;

pub const JOB_REGEX_SLURM1: &str = "/slurm/uid_(?<user_id__u64>[0-9]+)/job_(?<job_id__u64>[0-9]+)";
pub const JOB_REGEX_SLURM2: &str = "/slurmstepd.scope/job_(?<job_id__u64>[0-9]+)(?<remaining>(/.*)?)";

#[derive(Clone)]
struct JobDetails {
    step: Option<String>,
    sub_step: Option<String>,
    task: Option<String>,
}

pub fn find_jobid_in_attrs(attrs: &[(String, AttributeValue)]) -> Option<u64> {
    attrs.iter().find(|(k, _)| k == "job_id").map(|(_, v)| match v {
        AttributeValue::U64(id) => *id,
        _ => unreachable!("job_id should be a u64, is the regex correct?"),
    })
}

pub fn find_key_in_attrs(key: &str, attrs: &[(String, AttributeValue)]) -> Option<String> {
    attrs.iter().find(|(k, _)| k == key).map(|(_, v)| match v {
        AttributeValue::String(value) => value.clone(),
        _ => unreachable!("key: {} not found", key),
    })
}

fn extract_values(path: &str) -> Option<JobDetails> {
    // Split on "/" and ignore empty parts
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    if parts.is_empty() {
        return None;
    }
    let mut out: JobDetails = JobDetails {
        step: None,
        sub_step: None,
        task: None,
    };
    let element_len = parts.len();

    if element_len >= 1 {
        // First element : step_X -> X
        let step = parts[0];
        let num = step.strip_prefix("step_")?;
        out.step = Some(num.to_string());
    }

    if element_len >= 2 {
        out.sub_step = Some(parts[1].to_string());
    }

    if element_len >= 3 {
        let last = parts[2];
        let cleaned = last.strip_prefix("task_").unwrap_or(last);
        out.task = Some(cleaned.to_string());
    }

    Some(out)
}

#[derive(Clone)]
pub struct JobTagger {
    extractor_v1: RegexAttributesExtrator,
    extractor_v2: RegexAttributesExtrator,
}

impl JobTagger {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            extractor_v1: RegexAttributesExtrator::new(JOB_REGEX_SLURM1)?,
            extractor_v2: RegexAttributesExtrator::new(JOB_REGEX_SLURM2)?,
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

        // Retrieve remaining
        let remaining = attrs
            .iter()
            .position(|(key, _)| key == "remaining")
            .map(|pos| attrs.remove(pos));
        let mut attributs: Option<JobDetails> = None;
        if let Some((_key, AttributeValue::String(tmp_str))) = remaining {
            attributs = extract_values(tmp_str.as_str());
        }
        if let Some(attributs) = attributs {
            if let Some(step) = attributs.step {
                attrs.push(("step".to_string(), AttributeValue::String(step)));
            }
            if let Some(sub_step) = attributs.sub_step {
                attrs.push(("sub_step".to_string(), AttributeValue::String(sub_step)));
            }
            if let Some(task) = attributs.task {
                attrs.push(("task".to_string(), AttributeValue::String(task)));
            }
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

    #[test]
    fn test_find_jobstep_in_attrs() {
        let attrs: Vec<(String, AttributeValue)> = vec![
            ("job_step".to_string(), AttributeValue::String("1910".to_string())),
            ("Saphira".to_string(), AttributeValue::String("Eragon".to_string())),
        ];
        assert_eq!(find_key_in_attrs("Saphira", &attrs), Some("Eragon".to_string()));
    }

    #[test]
    fn test_find_job_step_in_attrs_not_existing() {
        let attrs: Vec<(String, AttributeValue)> = vec![
            ("not_job_step".to_string(), AttributeValue::String("1512".to_string())),
            ("Glaedr".to_string(), AttributeValue::String("Oromis".to_string())),
        ];
        assert_eq!(find_key_in_attrs("Gustave Eiffel", &attrs), None);
    }

    #[test]
    fn test_find_job_step_in_empty_vec() {
        let attrs: Vec<(String, AttributeValue)> = vec![];
        assert_eq!(find_key_in_attrs("Eugène Delacroix", &attrs), None);
    }
}
