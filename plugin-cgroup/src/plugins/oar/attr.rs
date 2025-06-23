use alumet::measurement::AttributeValue;


pub const JOB_REGEX_OAR2: &str = "^/oar/(?<user>[a-zA-Z]+)_(?<job_id__u64>[0-9]+)";
pub const JOB_REGEX_OAR3: &str = "^/oar.slice/.*/oar-u(?<user_id__u64>[0-9]+)-j(?<job_id__u64>[0-9]+)";


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
