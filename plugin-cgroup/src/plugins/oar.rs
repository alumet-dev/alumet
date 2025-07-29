use std::{process::Command, time::Duration};

use alumet::{
    measurement::AttributeValue,
    pipeline::elements::source::trigger::TriggerSpec,
    plugin::{
        rust::{deserialize_config, serialize_config, AlumetPlugin},
        AlumetPluginStart, AlumetPostStart, ConfigTable,
    },
};
use anyhow::{anyhow, Context};
use serde::{Deserialize, Serialize};

use crate::probe::{
    creator::{self, CgroupProbeCreator},
    personalise::{Personalised, ProbePersonaliser, RegexAttributesExtrator, SourceSettings},
    AugmentedMetrics, Metrics,
};

const JOB_REGEX_OAR2: &str = "^/oar/(?<user>[a-zA-Z]+)_(?<job_id__u64>[0-9]+)";
const JOB_REGEX_OAR3: &str = "^/oar.slice/.*/oar-u(?<user_id__u64>[0-9]+)-j(?<job_id__u64>[0-9]+)";

/// Gathers metrics for OAR jobs.
///
/// Supports OAR2 and OAR3, on cgroup v1 or cgroup v2.
pub struct OarPlugin {
    config: Option<Config>,
    /// Intermediary state for startup.
    starting_state: Option<StartingState>,
    /// The probe creator that is running in the background. Dropping it will stop it.
    probe_creator: Option<CgroupProbeCreator>,
}

impl OarPlugin {
    pub fn new(config: Config) -> Self {
        Self {
            config: Some(config),
            probe_creator: None,
            starting_state: None,
        }
    }
}

impl AlumetPlugin for OarPlugin {
    fn name() -> &'static str {
        "oar"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config: Config = deserialize_config(config)?;
        Ok(Box::new(Self::new(config)))
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        let config = serialize_config(Config::default())?;
        Ok(Some(config))
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        let starting_state = StartingState {
            metrics: Metrics::create(alumet)?,
            info_extractor: JobInfoExtractor::new(self.config.take().unwrap())?,
            creator_config: creator::Config::default(),
        };
        self.starting_state = Some(starting_state);
        Ok(())
    }

    fn post_pipeline_start(&mut self, alumet: &mut AlumetPostStart) -> anyhow::Result<()> {
        // TODO(core) perhaps we could make the control handle available sooner, but return an error if called before the pipeline is ready?
        let s = self.starting_state.take().unwrap();
        let probe_creator =
            CgroupProbeCreator::new(s.creator_config, s.metrics, s.info_extractor, alumet.pipeline_control())
                .context("failed to init CgroupProbeCreator")?;
        self.probe_creator = Some(probe_creator);
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        drop(self.probe_creator.take().unwrap());
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    oar_version: OarVersion,
    #[serde(with = "humantime_serde")]
    poll_interval: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            oar_version: OarVersion::Oar3,
            poll_interval: Duration::from_secs(1),
        }
    }
}

struct StartingState {
    metrics: Metrics,
    info_extractor: JobInfoExtractor,
    creator_config: creator::Config,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OarVersion {
    Oar2,
    Oar3,
}

#[derive(Clone)]
struct JobInfoExtractor {
    extractor: RegexAttributesExtrator,
    username_from_userid: bool,
    trigger: TriggerSpec,
}

impl JobInfoExtractor {
    fn new(config: Config) -> anyhow::Result<Self> {
        let trigger = TriggerSpec::at_interval(config.poll_interval);
        match config.oar_version {
            OarVersion::Oar2 => Ok(Self {
                extractor: RegexAttributesExtrator::new(JOB_REGEX_OAR2)?,
                username_from_userid: false,
                trigger,
            }),
            OarVersion::Oar3 => Ok(Self {
                extractor: RegexAttributesExtrator::new(JOB_REGEX_OAR3)?,
                username_from_userid: true,
                trigger,
            }),
        }
    }
}

impl ProbePersonaliser for JobInfoExtractor {
    fn personalise(&mut self, cgroup: &util_cgroups::Cgroup<'_>, metrics: &Metrics) -> Personalised {
        // extracts attributes "job_id" and ("user" or "user_id")
        let mut attrs = self
            .extractor
            .extract(cgroup.canonical_path())
            .expect("bad regex: it should only match if the input can be parsed into the specified types");

        if self.username_from_userid && !attrs.is_empty() {
            // Generate attribute "user" from "user_id".
            let user_id = find_userid_in_attrs(&attrs).expect("user_id should exist if username_from_userid is set");
            let user = username_from_id(user_id).unwrap(); // TODO handle error here
            attrs.push((String::from("user"), AttributeValue::String(user)));
        }

        let name = if attrs.is_empty() {
            // not a job, just a cgroup
            format!("cgroup {}", cgroup.unique_name())
        } else {
            format!(
                "oar-job-{}",
                find_jobid_in_attrs(&attrs).expect("job_id should always be set")
            )
        };
        let trigger = self.trigger.clone();
        let source_settings = SourceSettings { name, trigger };
        let metrics = AugmentedMetrics::with_common_attr_vec(metrics, attrs);
        Personalised {
            metrics,
            source_settings,
        }
    }
}

fn find_userid_in_attrs(attrs: &Vec<(String, AttributeValue)>) -> Option<u64> {
    attrs.iter().find(|(k, _)| k == "user_id").map(|(_, v)| match v {
        AttributeValue::U64(id) => *id,
        _ => unreachable!("user_id should be a u64, is the regex correct?"),
    })
}

fn find_jobid_in_attrs(attrs: &Vec<(String, AttributeValue)>) -> Option<u64> {
    attrs.iter().find(|(k, _)| k == "job_id").map(|(_, v)| match v {
        AttributeValue::U64(id) => *id,
        _ => unreachable!("job_id should be a u64, is the regex correct?"),
    })
}

fn username_from_id(id: u64) -> anyhow::Result<String> {
    let child = Command::new("id")
        .args(&["-n", "-u", &id.to_string()])
        .spawn()
        .context("failed to spawn process id")?;
    let output = child
        .wait_with_output()
        .context("failed to wait for process id to terminate")?;
    if !output.status.success() {
        let error_message = String::from_utf8_lossy(&output.stderr).into_owned();
        return Err(anyhow!("process id failed with {}", output.status).context(error_message));
    }
    let username = String::from_utf8_lossy(&output.stdout).into_owned();
    Ok(username)
}
