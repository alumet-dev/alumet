use alumet::measurement::AttributeValue;
use util_cgroups_plugins::{job_annotation_transform::JobTagger, regex::RegexAttributesExtrator};

// Not using `^` (which matches the beginning of the string) so that it works for a non-root OAR setup.
pub const JOB_REGEX_OAR2: &str = "/oar/(?<user>[a-zA-Z]+)_(?<job_id__u64>[0-9]+)";
pub const JOB_REGEX_OAR3: &str = "/oar.slice/.*/oar-u(?<user_id__u64>[0-9]+)-j(?<job_id__u64>[0-9]+)";

#[derive(Clone)]
pub struct OarJobTagger {
    extractor_v2: RegexAttributesExtrator,
    extractor_v3: RegexAttributesExtrator,
}

impl OarJobTagger {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            extractor_v2: RegexAttributesExtrator::new(JOB_REGEX_OAR2)?,
            extractor_v3: RegexAttributesExtrator::new(JOB_REGEX_OAR3)?,
        })
    }
}

impl JobTagger for OarJobTagger {
    fn attributes_for_cgroup(&self, cgroup: &util_cgroups::Cgroup) -> Vec<(String, AttributeValue)> {
        let extractor = match cgroup.hierarchy().version() {
            util_cgroups::CgroupVersion::V1 => &self.extractor_v2,
            util_cgroups::CgroupVersion::V2 => &self.extractor_v3,
        };
        // extracts attributes "job_id" and ("user" or "user_id")
        let mut attrs = extractor
            .extract(cgroup.canonical_path())
            .expect("bad regex: it should only match if the input can be parsed into the specified types");
        
        attrs
    }
}

pub fn find_userid_in_attrs(attrs: &Vec<(String, AttributeValue)>) -> Option<u64> {
    attrs.iter().find(|(k, _)| k == "user_id").map(|(_, v)| match v {
        AttributeValue::U64(id) => *id,
        _ => unreachable!("user_id should be a u64, is the regex correct?"),
    })
}

pub fn find_jobid_in_attrs(attrs: &Vec<(String, AttributeValue)>) -> Option<u64> {
    attrs.iter().find(|(k, _)| k == "job_id").map(|(_, v)| match v {
        AttributeValue::U64(id) => *id,
        _ => unreachable!("job_id should be a u64, is the regex correct?"),
    })
}
