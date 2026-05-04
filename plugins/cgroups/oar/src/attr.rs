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
    use std::panic::catch_unwind;
    use util_cgroups::{Cgroup, CgroupHierarchy, CgroupVersion};

    const MOCK_ROOT_HIERARCHY: &str = "/tmp/cgroup";
    const MOCK_CONTROLLER: [&str; 2] = ["cpu", "memory"];
    const MOCK_USER_ID: u64 = 10;
    const MOCK_JOB_ID: u64 = 123456;

    #[test]
    fn test_attributes_for_cgroup_with_oar2() {
        let hierarchy =
            CgroupHierarchy::manually_unchecked(MOCK_ROOT_HIERARCHY, CgroupVersion::V1, vec![MOCK_CONTROLLER[0]]);
        let cgroup = Cgroup::from_cgroup_path(&hierarchy, "/oar/user_10".to_owned());

        let mut tagger = OarJobTagger::new().unwrap();
        let attrs = tagger.attributes_for_cgroup(&cgroup);

        let job_id = attrs.iter().find(|(k, _)| k == "job_id").and_then(|(_, v)| match v {
            AttributeValue::U64(v) => Some(*v),
            _ => None,
        });
        let user = attrs.iter().find(|(k, _)| k == "user").and_then(|(_, v)| match v {
            AttributeValue::String(v) => Some(v.as_str()),
            _ => None,
        });

        assert_eq!(job_id, Some(10));
        assert_eq!(user, Some("user"));
    }

    #[test]
    fn test_attributes_for_cgroup_with_oar3() {
        let hierarchy =
            CgroupHierarchy::manually_unchecked(MOCK_ROOT_HIERARCHY, CgroupVersion::V2, vec![MOCK_CONTROLLER[1]]);
        let cgroup = Cgroup::from_cgroup_path(&hierarchy, format!("/oar.slice/system/oar-u10-j{}", MOCK_JOB_ID));

        let mut tagger = OarJobTagger::new().unwrap();
        let attrs = tagger.attributes_for_cgroup(&cgroup);

        let job_id = attrs.iter().find(|(k, _)| k == "job_id").and_then(|(_, v)| match v {
            AttributeValue::U64(v) => Some(*v),
            _ => None,
        });
        let user_id = attrs.iter().find(|(k, _)| k == "user_id").and_then(|(_, v)| match v {
            AttributeValue::U64(v) => Some(*v),
            _ => None,
        });

        assert_eq!(job_id, Some(123456));
        assert_eq!(user_id, Some(10));
    }

    #[test]
    fn test_find_userid_in_attrs_ok() {
        let attrs = vec![
            ("user_id".into(), AttributeValue::U64(MOCK_USER_ID)),
            ("job_id".into(), AttributeValue::U64(MOCK_JOB_ID)),
        ];

        let uid = find_userid_in_attrs(&attrs);
        assert_eq!(uid, Some(10));
    }

    #[test]
    fn test_find_userid_in_attrs_with_invalid_type() {
        let attrs = vec![("user_id".into(), AttributeValue::String("invalid".into()))];
        let result = catch_unwind(|| find_userid_in_attrs(&attrs));
        assert!(result.is_err());
    }

    #[test]
    fn test_find_jobid_in_attrs_ok() {
        let attrs = vec![
            ("user_id".into(), AttributeValue::U64(MOCK_USER_ID)),
            ("job_id".into(), AttributeValue::U64(MOCK_JOB_ID)),
        ];

        let jid = find_jobid_in_attrs(&attrs);
        assert_eq!(jid, Some(123456));
    }

    #[test]
    fn test_find_jobid_in_attrs_with_invalid_type() {
        let attrs = vec![("job_id".into(), AttributeValue::String("invalid".into()))];
        let result = catch_unwind(|| find_jobid_in_attrs(&attrs));
        assert!(result.is_err());
    }
}
