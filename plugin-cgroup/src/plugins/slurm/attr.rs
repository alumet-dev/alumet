use alumet::measurement::AttributeValue;


pub const JOB_REGEX_SLURM1: &str = "^/oar/(?<user>[a-zA-Z]+)_(?<job_id__u64>[0-9]+)";
pub const JOB_REGEX_SLURM2: &str = "^/system.slice/slurmstepd.scope/job_(?<job_id__u64>[0-9]+)";

pub fn find_jobid_in_attrs(attrs: &Vec<(String, AttributeValue)>) -> Option<u64> {
    attrs.iter().find(|(k, _)| k == "job_id").map(|(_, v)| match v {
        AttributeValue::U64(id) => *id,
        _ => unreachable!("job_id should be a u64, is the regex correct?"),
    })
}