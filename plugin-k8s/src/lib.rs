use alumet::{
    pipeline::{runtime::ControlHandle, trigger::TriggerSpec},
    plugin::{
        rust::{deserialize_config, serialize_config, AlumetPlugin},
        util::CounterDiff,
        ConfigTable, Plugin,
    },
};
use anyhow::Context;
use gethostname::gethostname;
use cgroupv2_utils::Metrics;
use notify::{Event, EventHandler, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::{fs::File, path::PathBuf, time::Duration};

mod k8s_cgroup_v2;
mod k8s_plugin;
mod k8s_probe;
mod cgroupv2_utils;

pub(crate) const CGROUP_MAX_TIME_COUNTER: u64 = u64::MAX;



