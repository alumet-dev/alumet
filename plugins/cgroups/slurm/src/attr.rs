use alumet::measurement::AttributeValue;
use const_format::formatcp;
use const_format::formatcp;
use util_cgroups::Cgroup;
use util_cgroups_plugins::job_annotation_transform::JobTagger;
use util_cgroups_plugins::regex::RegexAttributesExtrator;

const SLURM_TASK: &str = "task_(?<task_id>[^/]+)";
const SLURM_SUBSTEP: &str = "(?<substep_id>[^/]+)";
const SLURM_STEP: &str = "step_(?<step_id>[^/]+)";
const SLURM_JOB: &str = "job_(?<job_id__u64>[0-9]+)"; // TODO support the new SLUID as an alternative to the job id
pub const JOB_REGEX_SLURM1: &str = formatcp!("/slurm/uid_(?<user_id__u64>[0-9]+)/{SLURM_JOB}");
pub const JOB_REGEX_SLURM2: &str =
    formatcp!("/slurmstepd.scope/{SLURM_JOB}(/{SLURM_STEP}(/{SLURM_SUBSTEP}(/{SLURM_TASK})?)?)?");

const PSSLURM_USER: &str = "user-(?<user>[^/]+)";
const PSSLURM_JOB: &str = "job-(?<job_id__u64>[0-9]+)";
const PSSLURM_STEP: &str = "step-(?<step_id>[^/]+)";
pub const JOB_REGEX_PSSLURM: &str = formatcp!("/slurm/{PSSLURM_USER}/{PSSLURM_JOB}(/{PSSLURM_STEP})?");

#[derive(Clone)]
pub enum JobCgroupRegexExtractorStrategy {
    /// Parses Slurm cgroup paths. Detects cgroup v1 and cgroup v2 automatically.
    SlurmAuto {
        v1: RegexAttributesExtrator,
        v2: RegexAttributesExtrator,
    },

    /// Uses a fixed regex extractor.
    Fixed(RegexAttributesExtrator),
}

#[derive(Clone)]
pub struct SlurmJobTagger {
    strategy: JobCgroupRegexExtractorStrategy,
}

impl SlurmJobTagger {
    pub fn new(strategy: JobCgroupRegexExtractorStrategy) -> Self {
        Self { strategy }
    }
}

impl JobTagger for SlurmJobTagger {
    fn attributes_for_cgroup(&mut self, cgroup: &Cgroup) -> Vec<(String, AttributeValue)> {
        // extracts attributes "job_id" and ("user" or "user_id") and maybe others like step or sub-step
        let extractor = match &self.strategy {
            JobCgroupRegexExtractorStrategy::SlurmAuto { v1, v2 } => match cgroup.hierarchy().version() {
                util_cgroups::CgroupVersion::V1 => v1,
                util_cgroups::CgroupVersion::V2 => v2,
            },
            JobCgroupRegexExtractorStrategy::Fixed(ex) => ex,
        };

        extractor
            .extract(cgroup.canonical_path())
            .expect("bad regex: it should only match if the input can be parsed into the specified types")
    }
pub struct SlurmJobTagger {
    strategy: JobCgroupRegexExtractorStrategy,
}

impl SlurmJobTagger {
    pub fn new(strategy: JobCgroupRegexExtractorStrategy) -> Self {
        Self { strategy }
    }
}

impl JobTagger for SlurmJobTagger {
    fn attributes_for_cgroup(&mut self, cgroup: &Cgroup) -> Vec<(String, AttributeValue)> {
        // extracts attributes "job_id" and ("user" or "user_id") and maybe others like step or sub-step
        let extractor = match &self.strategy {
            JobCgroupRegexExtractorStrategy::SlurmAuto { v1, v2 } => match cgroup.hierarchy().version() {
                util_cgroups::CgroupVersion::V1 => v1,
                util_cgroups::CgroupVersion::V2 => v2,
            },
            JobCgroupRegexExtractorStrategy::Fixed(ex) => ex,
        };

        extractor
            .extract(cgroup.canonical_path())
            .expect("bad regex: it should only match if the input can be parsed into the specified types")
    }
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use std::path::PathBuf;

    use crate::attr::*;
    use alumet::measurement::AttributeValue;
    use util_cgroups::{CgroupHierarchy, CgroupVersion};
    use util_cgroups::{CgroupHierarchy, CgroupVersion};

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
    #[should_panic]
    fn test_find_jobid_in_attrs_not_u64() {
        let attrs: Vec<(String, AttributeValue)> = vec![
            ("job_id".to_string(), AttributeValue::String("19".to_string())),
            ("Saphira".to_string(), AttributeValue::String("Eragon".to_string())),
        ];
        find_jobid_in_attrs(&attrs);
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

    #[test]
    fn tagger_slurm_cgroupv1() {
        let h = CgroupHierarchy::manually_unchecked_v1_named(PathBuf::from("/sys/fs/cgroup"), "test".to_string());

        let job_cgroup = Cgroup::from_cgroup_path(&h, "/slurm/uid_123/job_999".to_string());
        let mut tagger = SlurmJobTagger::new(JobCgroupRegexExtractorStrategy::Fixed(
            RegexAttributesExtrator::new(JOB_REGEX_SLURM1).unwrap(),
        ));
        assert_eq!(
            tagger.attributes_for_cgroup(&job_cgroup),
            vec![
                ("user_id".to_string(), AttributeValue::U64(123)),
                ("job_id".to_string(), AttributeValue::U64(999))
            ]
        );
    fn tagger_slurm_cgroupv1() {
        let h = CgroupHierarchy::manually_unchecked_v1_named(PathBuf::from("/sys/fs/cgroup"), "test".to_string());

        let job_cgroup = Cgroup::from_cgroup_path(&h, "/slurm/uid_123/job_999".to_string());
        let mut tagger = SlurmJobTagger::new(JobCgroupRegexExtractorStrategy::Fixed(
            RegexAttributesExtrator::new(JOB_REGEX_SLURM1).unwrap(),
        ));
        assert_eq!(
            tagger.attributes_for_cgroup(&job_cgroup),
            vec![
                ("user_id".to_string(), AttributeValue::U64(123)),
                ("job_id".to_string(), AttributeValue::U64(999))
            ]
        );
    }

    #[test]
    fn tagger_slurm_cgroupv2() {
        let h = CgroupHierarchy::manually_unchecked(
            PathBuf::from("/sys/fs/cgroup"),
            CgroupVersion::V2,
            vec![String::from("cpuset")],
        );

        let job_cgroup = Cgroup::from_cgroup_path(&h, "/slurmstepd.scope/job_999".to_string());
        let mut tagger = SlurmJobTagger::new(JobCgroupRegexExtractorStrategy::Fixed(
            RegexAttributesExtrator::new(JOB_REGEX_SLURM2).unwrap(),
        ));
        assert_eq!(
            tagger.attributes_for_cgroup(&job_cgroup),
            vec![("job_id".to_string(), AttributeValue::U64(999))]
        );

        let task_cgroup =
            Cgroup::from_cgroup_path(&h, "/slurmstepd.scope/job_999/step_0/mysubthing/task_0".to_string());
        let mut tagger = SlurmJobTagger::new(JobCgroupRegexExtractorStrategy::Fixed(
            RegexAttributesExtrator::new(JOB_REGEX_SLURM2).unwrap(),
        ));
        assert_eq!(
            tagger.attributes_for_cgroup(&task_cgroup),
            vec![
                ("job_id".to_string(), AttributeValue::U64(999)),
                ("step_id".to_string(), AttributeValue::String(String::from("0"))),
                (
                    "substep_id".to_string(),
                    AttributeValue::String(String::from("mysubthing"))
                ),
                ("task_id".to_string(), AttributeValue::String(String::from("0"))),
            ]
        );
    fn tagger_slurm_cgroupv2() {
        let h = CgroupHierarchy::manually_unchecked(
            PathBuf::from("/sys/fs/cgroup"),
            CgroupVersion::V2,
            vec![String::from("cpuset")],
        );

        let job_cgroup = Cgroup::from_cgroup_path(&h, "/slurmstepd.scope/job_999".to_string());
        let mut tagger = SlurmJobTagger::new(JobCgroupRegexExtractorStrategy::Fixed(
            RegexAttributesExtrator::new(JOB_REGEX_SLURM2).unwrap(),
        ));
        assert_eq!(
            tagger.attributes_for_cgroup(&job_cgroup),
            vec![("job_id".to_string(), AttributeValue::U64(999))]
        );

        let task_cgroup =
            Cgroup::from_cgroup_path(&h, "/slurmstepd.scope/job_999/step_0/mysubthing/task_0".to_string());
        let mut tagger = SlurmJobTagger::new(JobCgroupRegexExtractorStrategy::Fixed(
            RegexAttributesExtrator::new(JOB_REGEX_SLURM2).unwrap(),
        ));
        assert_eq!(
            tagger.attributes_for_cgroup(&task_cgroup),
            vec![
                ("job_id".to_string(), AttributeValue::U64(999)),
                ("step_id".to_string(), AttributeValue::String(String::from("0"))),
                (
                    "substep_id".to_string(),
                    AttributeValue::String(String::from("mysubthing"))
                ),
                ("task_id".to_string(), AttributeValue::String(String::from("0"))),
            ]
        );
    }

    #[test]
    fn tagger_psslurm_cgroupv2() {
        let h = CgroupHierarchy::manually_unchecked(
            PathBuf::from("/sys/fs/cgroup"),
            CgroupVersion::V2,
            vec![String::from("cpuset")],
        );

        let job_cgroup = Cgroup::from_cgroup_path(&h, "/slurm/user-guigui/job-2026".to_string());
        let mut tagger = SlurmJobTagger::new(JobCgroupRegexExtractorStrategy::Fixed(
            RegexAttributesExtrator::new(JOB_REGEX_PSSLURM).unwrap(),
        ));
        assert_eq!(
            tagger.attributes_for_cgroup(&job_cgroup),
            vec![
                ("user".to_string(), AttributeValue::String("guigui".to_string())),
                ("job_id".to_string(), AttributeValue::U64(2026))
            ]
        );

        let step_cgroup = Cgroup::from_cgroup_path(&h, "/slurm/user-guigui/job-2026/step-7".to_string());
        let mut tagger = SlurmJobTagger::new(JobCgroupRegexExtractorStrategy::Fixed(
            RegexAttributesExtrator::new(JOB_REGEX_PSSLURM).unwrap(),
        ));
        assert_eq!(
            tagger.attributes_for_cgroup(&step_cgroup),
            vec![
                ("user".to_string(), AttributeValue::String("guigui".to_string())),
                ("job_id".to_string(), AttributeValue::U64(2026)),
                ("step_id".to_string(), AttributeValue::String("7".to_string())),
            ]
        );
    }
}
