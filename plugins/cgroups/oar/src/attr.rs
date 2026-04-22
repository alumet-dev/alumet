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
    fn attributes_for_cgroup(&mut self, cgroup: &util_cgroups::Cgroup) -> Vec<(String, AttributeValue)> {
        let extractor = match cgroup.hierarchy().version() {
            util_cgroups::CgroupVersion::V1 => &self.extractor_v2,
            util_cgroups::CgroupVersion::V2 => &self.extractor_v3,
        };
        // extracts attributes "job_id" and ("user" or "user_id")
        let attrs = extractor
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

#[cfg(test)]
mod tests {
    use super::*;
    use util_cgroups::{Cgroup, CgroupHierarchy, CgroupVersion};

    const MOCK_ROOT_HIERARCHY: &str = "/tmp/cgroup";
    const MOCK_CONTROLLER: [&str; 2] = ["cpu", "memory"];
    const MOCK_ID: [u64; 2] = [10, 20];

    #[test]
    fn test_attributes_for_cgroup_with_oar2() {
        let hierarchy =
            CgroupHierarchy::manually_unchecked(MOCK_ROOT_HIERARCHY, CgroupVersion::V1, vec![MOCK_CONTROLLER[0]]);

        let cgroups = vec![
            Cgroup::from_cgroup_path(&hierarchy, format!("/oar/user_{}", MOCK_ID[0])), // Tracked job
            Cgroup::from_cgroup_path(&hierarchy, format!("/oar/user_{}", MOCK_ID[1])), // Not tracked job
            Cgroup::from_cgroup_path(&hierarchy, "/invalid/job".to_owned()),           // Invalid job
        ];

        let mut tagger = OarJobTagger::new().unwrap();
        let attrs = tagger.attributes_for_cgroup(&cgroups[0]);

        assert!(attrs.iter().any(|(k, _)| k == "job_id"));
        assert!(attrs.iter().any(|(k, _)| k == "user"));
    }

    #[test]
    fn test_attributes_for_cgroup_with_oar3() {
        let hierarchy =
            CgroupHierarchy::manually_unchecked(MOCK_ROOT_HIERARCHY, CgroupVersion::V2, vec![MOCK_CONTROLLER[1]]);

        let cgroups = vec![
            Cgroup::from_cgroup_path(&hierarchy, format!("/oar.slice/system/oar-u1-j{}", MOCK_ID[0])), // Tracked job
            Cgroup::from_cgroup_path(&hierarchy, format!("/oar.slice/system/oar-u1-j{}", MOCK_ID[1])), // Not tracked job
            Cgroup::from_cgroup_path(&hierarchy, "/invalid/job".to_owned()),                           // Invalid job
        ];

        let mut tagger = OarJobTagger::new().unwrap();
        let attrs = tagger.attributes_for_cgroup(&cgroups[0]);

        assert!(attrs.iter().any(|(k, _)| k == "job_id"));
        assert!(attrs.iter().any(|(k, _)| k == "user_id"));
    }

    #[test]
    fn test_find_userid_in_attrs_ok() {
        let attrs = vec![
            ("user_id".into(), AttributeValue::U64(54)),
            ("job_id".into(), AttributeValue::U64(123456)),
        ];

        let uid = find_userid_in_attrs(&attrs);
        assert_eq!(uid, Some(54));
    }

    #[test]
    #[should_panic(expected = "user_id should be a u64, is the regex correct?")]
    fn test_find_userid_in_attrs_with_invalid_type() {
        let attrs = vec![("user_id".into(), AttributeValue::String("invalid".into()))];
        let _ = find_userid_in_attrs(&attrs);
    }

    #[test]
    fn test_find_jobid_in_attrs_ok() {
        let attrs = vec![
            ("user_id".into(), AttributeValue::U64(54)),
            ("job_id".into(), AttributeValue::U64(123456)),
        ];

        let jid = find_jobid_in_attrs(&attrs);
        assert_eq!(jid, Some(123456));
    }

    #[test]
    #[should_panic(expected = "job_id should be a u64, is the regex correct?")]
    fn test_find_jobid_in_attrs_with_invalid_type() {
        let attrs = vec![("job_id".into(), AttributeValue::String("invalid".into()))];
        let _ = find_jobid_in_attrs(&attrs);
    }
}
