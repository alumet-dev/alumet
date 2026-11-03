//! Extract FLOPS and memory-volume groups from the LIKWID source tree.
//!
//! Architecture names and CPU constants come from topology.c/topology.h; event-table
//! selection follows perfmon.c for the architecture's first supported model and stepping.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
    sync::LazyLock,
};

use anyhow::{Context, anyhow, bail};
use chrono::{SecondsFormat, Utc};
use clap::Parser;
use indexmap::IndexMap;
use regex::Regex;
use serde::Serialize;

const DEFAULT_REPO: &str = "https://github.com/RRZE-HPC/likwid.git";
const DEFAULT_BRANCH: &str = "master";
const DEFAULT_OUTPUT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data/likwid_metrics.json");
const FLOPS_UNITS: &str = "FLOP";
const MEMORY_UNITS: &str = "Bytes";
const NOT_FOUND: &str = "not found";

macro_rules! regex {
    ($name:ident, $re:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($re).unwrap());
    };
}

regex!(FLOPS_FILE_RE, r"^FLOPS(?:_[A-Za-z0-9]+)?\.txt$");
regex!(MEMORY_FILE_RE, r"^MEM(?:[0-9]+)?\.txt$");
regex!(MEMORY_NAME_RE, r"(?i)\b(memory|dram)\b");
regex!(MEMORY_VOLUME_RE, r"(?i)\b(volume|data volume)\b");
regex!(EXCLUDED_MEMORY_RE, r"(?i)\b(cache|cpi|latency|utili[sz]|miss|hit)\b");
regex!(FLOP_WORD_RE, r"(?i)FLOP");
regex!(UNITS_SUFFIX_RE, r"\s+\[([^\]]+)\]$");
regex!(SCALE_FACTOR_RE, r"\b1(?:\.0+)?[Ee]-0*[69]\s*\*\s*");
regex!(TIME_DIVISION_RE, r"(?i)/\s*time");
regex!(ASSUMED_ANNOTATION_RE, r"(?i)\([^()]*\bassumed\b\)\s*");
regex!(
    EVENT_TABLE_LINE_RE,
    r"^\s*(EVENT|UMASK)_([A-Za-z0-9_]+)\s+(0x[0-9A-Fa-f]+)(?:\s+\S+)*\s*(?:#.*)?$"
);
regex!(METRIC_LINE_RE, r"^(.+?)\s+(\[[^\]]+\])\s+(.+)$");
regex!(DEFINE_RE, r"(?m)^\s*#define\s+(\w+)\s+(0x[0-9A-Fa-f]+|\d+)U?\s*$");
regex!(NAME_VALUE_RE, r#"static const char \*([A-Za-z0-9_]+)\s*=\s*"([^"]+)";"#);
regex!(SET_NAME_FN_RE, r"int topology_setName\(void\)");
regex!(FAMILY_CASE_RE, r"^\s{4}case\s+(\w+)\s*:");
regex!(CASE_RE, r"\bcase\s+(\w+)\s*:");
regex!(STEPPING_RE, r"if\s*\(\s*cpuid_info\.stepping\s*([<>]=?)\s*(\d+)\s*\)");
regex!(ELSE_RE, r"\belse\b");
regex!(NAME_ASSIGN_RE, r"\bname\s*=\s*(\w+)\s*;");
regex!(SHORT_NAME_ASSIGN_RE, r"\bshort_name\s*=\s*(\w+)\s*;");
regex!(PERFMON_MAPS_FN_RE, r"\bperfmon_init_maps\s*\(\s*void\s*\)");
regex!(EVENT_TABLE_ASSIGN_RE, r"\beventHash\s*=\s*(\w+)_arch_events\s*;");
regex!(BREAK_RE, r"\bbreak\s*;");
regex!(C_COMMENT_RE, r"(?s)/\*.*?\*/|//[^\n]*");

#[derive(Parser, Debug)]
#[command(about = "Extract FLOPS and memory-volume groups from the LIKWID source tree.")]
struct Args {
    /// LIKWID Git repository URL
    #[arg(long, default_value = DEFAULT_REPO)]
    repo: String,
    /// Git branch or tag to retrieve
    #[arg(long, default_value = DEFAULT_BRANCH)]
    branch: String,
    /// Output JSON path
    #[arg(long, default_value = DEFAULT_OUTPUT)]
    output: PathBuf,
    /// Keep the downloaded checkout and print its path
    #[arg(long)]
    keep_workdir: bool,
    /// Enable debug logging
    #[arg(long)]
    verbose: bool,
}

#[derive(Serialize, Debug)]
struct GroupMetric {
    name: String,
    units: String,
    formula: String,
    events: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    unresolved_dependencies: Vec<String>,
}

struct ParsedGroup {
    eventset: BTreeMap<String, String>,
    metrics: Vec<(String, String)>,
}

struct EventTable {
    events: HashMap<String, String>,
    umasks: HashMap<String, String>,
    umask_events: HashMap<String, String>,
}

#[derive(Clone, PartialEq, Debug)]
struct ModelDefinition {
    family_constant: Option<String>,
    family: Option<u64>,
    model_name: String,
    model_code: u64,
    condition: Option<String>,
}

struct ArchitectureMapping {
    model_definitions: Vec<ModelDefinition>,
    architecture_names: Vec<String>,
}

struct EventTableDefinition {
    family_constant: String,
    model_names: Vec<String>,
    condition: Option<String>,
    table_name: String,
}

#[derive(Serialize)]
struct OutputModel {
    model_name: String,
    model_code: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    condition: Option<String>,
}

#[derive(Serialize)]
struct ArchitectureDefinitions {
    family_constant: Option<String>,
    models: Vec<OutputModel>,
    architecture_full_name: Option<String>,
    architecture_abbreviation: String,
}

#[derive(Serialize)]
struct ArchitectureMetrics {
    flops: Vec<GroupMetric>,
    memory_volume: Vec<GroupMetric>,
}

#[derive(Serialize)]
struct ArchitectureRecord {
    definitions: ArchitectureDefinitions,
    metrics: ArchitectureMetrics,
}

#[derive(Serialize)]
struct SourceInfo {
    repository: String,
    groups_path: String,
    branch: String,
    commit: String,
    generated_at: String,
}

#[derive(Serialize)]
struct Output {
    source: SourceInfo,
    architectures: IndexMap<String, Vec<ArchitectureRecord>>,
}

/// A cloned LIKWID checkout. The temporary directory is deleted on drop unless it was kept.
struct Checkout {
    root: PathBuf,
    commit: String,
    _tempdir: Option<tempfile::TempDir>,
    kept_workdir: Option<PathBuf>,
}

/// Clone the requested branch into a temporary directory and return its commit.
fn download_likwid_source(repo: &str, branch: &str, keep_workdir: bool) -> anyhow::Result<Checkout> {
    let tempdir = tempfile::Builder::new()
        .prefix("likwid-source-")
        .tempdir()
        .context("could not create temporary directory")?;
    let checkout = tempdir.path().join("likwid");

    let run = |cmd: &mut Command| -> anyhow::Result<String> {
        let output = cmd
            .output()
            .map_err(|e| anyhow!("Could not retrieve LIKWID from {repo}: {e}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Could not retrieve LIKWID from {repo}: {}", stderr.trim());
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    };

    run(Command::new("git")
        .args(["clone", "--depth", "1", "--branch", branch, repo])
        .arg(&checkout))?;
    let commit = run(Command::new("git").arg("-C").arg(&checkout).args(["rev-parse", "HEAD"]))?;
    log::info!("Downloaded LIKWID commit {commit}");

    let (tempdir, kept_workdir) = if keep_workdir {
        (None, Some(tempdir.keep()))
    } else {
        (Some(tempdir), None)
    };
    Ok(Checkout {
        root: checkout,
        commit,
        _tempdir: tempdir,
        kept_workdir,
    })
}

fn parse_int(value: &str) -> anyhow::Result<u64> {
    let parsed = match value.strip_prefix("0x").or_else(|| value.strip_prefix("0X")) {
        Some(hex) => u64::from_str_radix(hex, 16),
        None => value.parse(),
    };
    parsed.with_context(|| format!("invalid integer constant: {value}"))
}

/// Parse LIKWID's model constants and `topology_setName` source mapping.
///
/// The mapping intentionally comes from LIKWID source rather than a hard-coded table.
/// LIKWID's `short_name` is the architecture code used by the groups directory.
fn parse_likwid_architecture_mapping(source_root: &Path) -> anyhow::Result<BTreeMap<String, ArchitectureMapping>> {
    let header = read_to_string(&source_root.join("src/includes/topology.h"))?;
    let topology = read_to_string(&source_root.join("src/topology.c"))?;
    let header = C_COMMENT_RE.replace_all(&header, "");
    let topology = C_COMMENT_RE.replace_all(&topology, "");

    let mut constants = HashMap::new();
    for c in DEFINE_RE.captures_iter(&header) {
        constants.insert(c[1].to_owned(), parse_int(&c[2])?);
    }
    let name_values: HashMap<&str, &str> = NAME_VALUE_RE
        .captures_iter(&topology)
        .map(|c| (c.get(1).unwrap().as_str(), c.get(2).unwrap().as_str()))
        .collect();

    // Function body: everything up to the next top-level `int ` or `void ` definition.
    let fn_start = SET_NAME_FN_RE
        .find(&topology)
        .ok_or_else(|| anyhow!("LIKWID topology_setName function was not found"))?
        .end();
    let rest = &topology[fn_start..];
    let fn_end = [rest.find("\nint "), rest.find("\nvoid ")]
        .into_iter()
        .flatten()
        .min()
        .ok_or_else(|| anyhow!("LIKWID topology_setName function was not found"))?;
    let body = &rest[..fn_end];

    let mut mappings: BTreeMap<String, ArchitectureMapping> = BTreeMap::new();
    let mut pending_cases: Vec<String> = Vec::new();
    let mut previous_cases: Vec<String> = Vec::new();
    let mut pending_display_name = String::new();
    let mut branch_condition: Option<String> = None;
    let mut inverse_condition: Option<String> = None;
    let mut family_constant: Option<String> = None;

    for line in body.lines() {
        if let Some(c) = FAMILY_CASE_RE.captures(line) {
            family_constant = Some(c[1].to_owned());
        }
        if let Some(c) = CASE_RE.captures(line) {
            if pending_cases.is_empty() {
                branch_condition = None;
                inverse_condition = None;
            }
            pending_cases.push(c[1].to_owned());
        }
        if let Some(c) = STEPPING_RE.captures(line) {
            let (condition, inverse) = stepping_conditions(&c[1], &c[2]);
            branch_condition = Some(condition);
            inverse_condition = Some(inverse);
        } else if ELSE_RE.is_match(line) && branch_condition.is_some() {
            std::mem::swap(&mut branch_condition, &mut inverse_condition);
        }
        if let Some(c) = NAME_ASSIGN_RE.captures(line) {
            pending_display_name = name_values.get(&c[1]).copied().unwrap_or_default().to_owned();
        }
        let Some(short) = SHORT_NAME_ASSIGN_RE.captures(line) else {
            continue;
        };
        let cases = if pending_cases.is_empty() {
            previous_cases.clone()
        } else {
            std::mem::take(&mut pending_cases)
        };
        if let Some(code) = name_values.get(&short[1]).filter(|c| !c.is_empty()) {
            let entry = mappings
                .entry((*code).to_owned())
                .or_insert_with(|| ArchitectureMapping {
                    model_definitions: Vec::new(),
                    architecture_names: Vec::new(),
                });
            for constant in &cases {
                if let Some(&model_code) = constants.get(constant) {
                    let definition = ModelDefinition {
                        family_constant: family_constant.clone(),
                        family: family_constant.as_ref().and_then(|f| constants.get(f).copied()),
                        model_name: constant.clone(),
                        model_code,
                        condition: branch_condition.clone(),
                    };
                    if !entry.model_definitions.contains(&definition) {
                        entry.model_definitions.push(definition);
                    }
                }
            }
            if !pending_display_name.is_empty() && !entry.architecture_names.contains(&pending_display_name) {
                entry.architecture_names.push(pending_display_name.clone());
            }
        }
        previous_cases = cases;
        pending_cases.clear();
        pending_display_name.clear();
    }
    Ok(mappings)
}

fn stepping_conditions(operator: &str, threshold: &str) -> (String, String) {
    let inverse_operator = match operator {
        "<" => ">=",
        "<=" => ">",
        ">" => "<=",
        ">=" => "<",
        _ => unreachable!("STEPPING_RE only matches comparison operators"),
    };
    (
        format!("stepping {operator} {threshold}"),
        format!("stepping {inverse_operator} {threshold}"),
    )
}

/// Read LIKWID's actual table selections, including shared tables and stepping branches.
fn parse_likwid_event_tables(source_root: &Path) -> anyhow::Result<Vec<EventTableDefinition>> {
    let perfmon = read_to_string(&source_root.join("src/perfmon.c"))?;
    let perfmon = C_COMMENT_RE.replace_all(&perfmon, "");
    let start = PERFMON_MAPS_FN_RE
        .find(&perfmon)
        .ok_or_else(|| anyhow!("LIKWID perfmon_init_maps function was not found"))?
        .end();
    let rest = &perfmon[start..];
    let end = rest
        .find("\n}")
        .ok_or_else(|| anyhow!("LIKWID perfmon_init_maps function end was not found"))?;
    let mut definitions = Vec::new();
    let mut family_constant = None;
    let mut model_names = Vec::new();
    let mut condition = None;
    let mut inverse_condition = None;

    for line in rest[..end].lines() {
        if let Some(c) = CASE_RE.captures(line) {
            if c[1].ends_with("_FAMILY") {
                family_constant = Some(c[1].to_owned());
                model_names.clear();
                condition = None;
                inverse_condition = None;
            } else {
                model_names.push(c[1].to_owned());
            }
        }
        if let Some(c) = STEPPING_RE.captures(line) {
            let (branch, inverse) = stepping_conditions(&c[1], &c[2]);
            condition = Some(branch);
            inverse_condition = Some(inverse);
        } else if ELSE_RE.is_match(line) && condition.is_some() {
            std::mem::swap(&mut condition, &mut inverse_condition);
        }
        if let Some(c) = EVENT_TABLE_ASSIGN_RE.captures(line) {
            let family_constant = family_constant
                .as_ref()
                .ok_or_else(|| anyhow!("Event table {} has no CPU family in perfmon_init_maps", &c[1]))?;
            definitions.push(EventTableDefinition {
                family_constant: family_constant.clone(),
                model_names: model_names.clone(),
                condition: condition.clone(),
                table_name: c[1].to_owned(),
            });
        }
        if BREAK_RE.is_match(line) {
            model_names.clear();
            condition = None;
            inverse_condition = None;
        }
    }
    if definitions.is_empty() {
        bail!("No event-table selections found in LIKWID perfmon_init_maps");
    }
    Ok(definitions)
}

fn read_to_string(path: &Path) -> anyhow::Result<String> {
    fs::read_to_string(path).with_context(|| format!("could not read {}", path.display()))
}

/// Parse one `counter event_name` pair or one `name [unit] formula` metric per section line.
fn parse_group_file(content: &str) -> ParsedGroup {
    let mut section = "";
    let mut eventset = BTreeMap::new();
    let mut metrics = Vec::new();
    let lines = content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with('-'));
    for line in lines {
        let upper = line.to_uppercase();
        match upper.as_str() {
            "EVENTSET" => section = "EVENTSET",
            "METRICS" => section = "METRICS",
            "LONG" => section = "LONG",
            _ if upper.starts_with("FORMULAS:") => section = "LONG",
            _ if section == "EVENTSET" => {
                let fields: Vec<&str> = line.split_whitespace().collect();
                if let [counter, event_name] = fields[..] {
                    eventset.insert(counter.to_owned(), event_name.to_owned());
                }
            }
            _ if section == "METRICS" => {
                if let Some(c) = METRIC_LINE_RE.captures(line) {
                    metrics.push((format!("{} {}", c[1].trim(), &c[2]), c[3].trim().to_owned()));
                }
            }
            _ => {}
        }
    }
    ParsedGroup { eventset, metrics }
}

/// Find the table selected by LIKWID for the architecture's first supported CPU model.
fn find_event_table(
    source_root: &Path,
    architecture: &str,
    mapping: Option<&ArchitectureMapping>,
    definitions: &[EventTableDefinition],
) -> anyhow::Result<Option<PathBuf>> {
    let table_architecture = mapping.and_then(|mapping| {
        mapping.model_definitions.iter().find_map(|model| {
            definitions
                .iter()
                .find(|definition| {
                    model.family_constant.as_ref() == Some(&definition.family_constant)
                        && (definition.model_names.is_empty() || definition.model_names.contains(&model.model_name))
                        && (definition.condition.is_none() || definition.condition == model.condition)
                })
                .map(|definition| definition.table_name.as_str())
        })
    });
    let Some(table_architecture) = table_architecture else {
        log::warn!("Could not resolve event-table selection for groups/{architecture}");
        return Ok(None);
    };
    let expected_name = format!("perfmon_{table_architecture}_events.txt").to_lowercase();
    let includes = source_root.join("src/includes");
    let mut matches: Vec<PathBuf> = fs::read_dir(&includes)
        .with_context(|| format!("could not list {}", includes.display()))?
        .collect::<Result<Vec<_>, _>>()
        .with_context(|| format!("could not read entries in {}", includes.display()))?
        .into_iter()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                n.starts_with("perfmon_") && n.ends_with("_events.txt") && n.to_lowercase() == expected_name
            })
        })
        .collect();
    matches.sort();
    let found = matches.into_iter().next();
    if found.is_none() {
        log::warn!("Could not find {expected_name} for groups/{architecture}");
    }
    Ok(found)
}

/// Parse EVENT and UMASK declarations from a LIKWID event table.
fn parse_event_table(path: &Path) -> anyhow::Result<EventTable> {
    let mut table = EventTable {
        events: HashMap::new(),
        umasks: HashMap::new(),
        umask_events: HashMap::new(),
    };
    let mut current_event = String::new();
    for line in read_to_string(path)?.lines() {
        let Some(c) = EVENT_TABLE_LINE_RE.captures(line) else {
            continue;
        };
        let name = c[2].to_owned();
        let hex_value = c[3][2..].to_lowercase();
        if &c[1] == "EVENT" {
            table.events.insert(name, hex_value.clone());
            current_event = hex_value;
        } else {
            table.umasks.insert(name.clone(), hex_value);
            if !current_event.is_empty() {
                table.umask_events.insert(name, current_event.clone());
            }
        }
    }
    Ok(table)
}

/// Compose `r<umask><event>` using the longest matching event name.
fn resolve_raw_counter(event_specification: &str, table: &EventTable) -> String {
    let event_name = event_specification.split(':').next().unwrap_or_default();
    if let (Some(umask), Some(event)) = (table.umasks.get(event_name), table.umask_events.get(event_name)) {
        return format!("r{umask}{event}");
    }
    let Some(umask) = table.umasks.get(event_name) else {
        return NOT_FOUND.to_owned();
    };
    let base_event = table
        .events
        .keys()
        .filter(|name| {
            event_name == name.as_str()
                || event_name
                    .strip_prefix(name.as_str())
                    .is_some_and(|rest| rest.starts_with('_'))
        })
        .max_by_key(|name| name.len());
    match base_event {
        Some(base) => format!("r{umask}{}", table.events[base]),
        None => NOT_FOUND.to_owned(),
    }
}

fn is_flops_metric(name: &str, formula: &str) -> bool {
    FLOP_WORD_RE.is_match(&format!("{name} {formula}"))
}

fn is_memory_volume_metric(name: &str, formula: &str) -> bool {
    let combined = format!("{name} {formula}");
    MEMORY_NAME_RE.is_match(&combined)
        && MEMORY_VOLUME_RE.is_match(&combined)
        && !EXCLUDED_MEMORY_RE.is_match(&combined)
}

fn split_metric_name(name: &str) -> anyhow::Result<(String, String)> {
    let c = UNITS_SUFFIX_RE
        .captures(name)
        .ok_or_else(|| anyhow!("Metric name has no trailing units: {name}"))?;
    let start = c.get(0).unwrap().start();
    Ok((name[..start].trim_end().to_owned(), c[1].to_owned()))
}

fn normalize_formula(formula: &str) -> String {
    let formula = SCALE_FACTOR_RE.replace_all(formula, "");
    let formula = TIME_DIVISION_RE.replace_all(&formula, "");
    ASSUMED_ANNOTATION_RE.replace_all(&formula, "").into_owned()
}

fn contains_word(haystack: &str, word: &str) -> bool {
    Regex::new(&format!(r"\b{}\b", regex::escape(word)))
        .map(|re| re.is_match(haystack))
        .unwrap_or(false)
}

/// Resolve eventset counters and event names referenced by a formula.
fn resolve_metric_dependencies(
    formula: &str,
    eventset: &BTreeMap<String, String>,
    event_table: Option<&EventTable>,
) -> (BTreeMap<String, String>, Vec<String>) {
    let mut counters: BTreeSet<&str> = eventset
        .keys()
        .filter(|counter| contains_word(formula, counter))
        .map(String::as_str)
        .collect();
    let mut event_to_counters: HashMap<&str, Vec<&str>> = HashMap::new();
    for (counter, event) in eventset {
        let event_name = event.split_whitespace().next().unwrap_or_default();
        event_to_counters.entry(event_name).or_default().push(counter);
    }
    for (event_name, event_counters) in &event_to_counters {
        if contains_word(formula, event_name) {
            counters.extend(event_counters);
        }
    }

    let mut events = BTreeMap::new();
    let mut unresolved = Vec::new();
    for counter in counters {
        let event_name = &eventset[counter];
        let raw_counter = match event_table {
            Some(table) => resolve_raw_counter(event_name, table),
            None => NOT_FOUND.to_owned(),
        };
        if raw_counter == NOT_FOUND {
            unresolved.push(event_name.clone());
        }
        events.insert(counter.to_owned(), raw_counter);
    }
    (events, unresolved)
}

fn select_metrics(
    group: &ParsedGroup,
    flops: bool,
    event_table: Option<&EventTable>,
) -> anyhow::Result<Vec<GroupMetric>> {
    let mut selected = Vec::new();
    for (name, formula) in &group.metrics {
        let wanted = if flops {
            is_flops_metric(name, formula)
        } else {
            is_memory_volume_metric(name, formula)
        };
        if !wanted {
            continue;
        }
        let (name, _source_units) = split_metric_name(name)?;
        let formula = normalize_formula(formula);
        let (events, unresolved_dependencies) = resolve_metric_dependencies(&formula, &group.eventset, event_table);
        selected.push(GroupMetric {
            name,
            units: (if flops { FLOPS_UNITS } else { MEMORY_UNITS }).to_owned(),
            formula,
            events,
            unresolved_dependencies,
        });
    }
    Ok(selected)
}

fn sorted_entries(dir: &Path, want_dirs: bool) -> anyhow::Result<Vec<PathBuf>> {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .with_context(|| format!("could not list {}", dir.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| if want_dirs { p.is_dir() } else { p.is_file() })
        .collect();
    entries.sort();
    Ok(entries)
}

fn file_name(path: &Path) -> String {
    path.file_name().unwrap_or_default().to_string_lossy().into_owned()
}

fn build_architecture_records(
    source_root: &Path,
    mappings: &BTreeMap<String, ArchitectureMapping>,
    event_tables: &[EventTableDefinition],
) -> anyhow::Result<IndexMap<String, Vec<ArchitectureRecord>>> {
    let mut records: IndexMap<String, Vec<ArchitectureRecord>> = IndexMap::new();
    for directory in sorted_entries(&source_root.join("groups"), true)? {
        let arch = file_name(&directory);
        let mut flops = Vec::new();
        let mut memory = Vec::new();
        let event_table = match find_event_table(source_root, &arch, mappings.get(&arch), event_tables)? {
            Some(path) => Some(parse_event_table(&path)?),
            None => None,
        };
        for path in sorted_entries(&directory, false)? {
            let name = file_name(&path);
            let is_memory = MEMORY_FILE_RE.is_match(&name);
            if !is_memory && !FLOPS_FILE_RE.is_match(&name) {
                continue;
            }
            let parsed = match fs::read_to_string(&path) {
                Ok(content) => parse_group_file(&content),
                Err(e) => {
                    log::warn!("{arch}/{name}: could not parse group file: {e}");
                    continue;
                }
            };
            if is_memory {
                memory.extend(select_metrics(&parsed, false, event_table.as_ref())?);
            } else {
                flops.extend(select_metrics(&parsed, true, event_table.as_ref())?);
            }
        }
        if flops.is_empty() && memory.is_empty() {
            continue;
        }
        let Some(mapping) = mappings.get(&arch) else {
            log::warn!("Could not resolve architecture code for groups/{arch}");
            continue;
        };
        let Some(primary) = mapping.model_definitions.first() else {
            log::warn!("Could not resolve CPU family for groups/{arch}");
            continue;
        };
        let family_key = primary.family.map_or_else(|| "None".to_owned(), |f| f.to_string());
        let models = mapping
            .model_definitions
            .iter()
            .map(|d| OutputModel {
                model_name: d.model_name.clone(),
                model_code: d.model_code,
                condition: d.condition.clone(),
            })
            .collect();
        log::info!(
            "Architecture: {arch} (FLOPS metrics: {}, memory-volume metrics: {})",
            flops.len(),
            memory.len()
        );
        records.entry(family_key).or_default().push(ArchitectureRecord {
            definitions: ArchitectureDefinitions {
                family_constant: primary.family_constant.clone(),
                models,
                architecture_full_name: mapping.architecture_names.first().cloned(),
                architecture_abbreviation: arch,
            },
            metrics: ArchitectureMetrics {
                flops,
                memory_volume: memory,
            },
        });
    }
    Ok(records)
}

fn validate_output(data: &Output) -> anyhow::Result<()> {
    for (family_code, architectures) in &data.architectures {
        for architecture in architectures {
            let arch_code = &architecture.definitions.architecture_abbreviation;
            let categories = [
                ("flops", &architecture.metrics.flops),
                ("memory_volume", &architecture.metrics.memory_volume),
            ];
            for (_category, metrics) in categories {
                for metric in metrics {
                    if metric.units.is_empty() {
                        bail!("Empty units in {family_code}/{arch_code}/{}", metric.name);
                    }
                    if metric.formula.is_empty() {
                        bail!("Empty formula in {family_code}/{arch_code}/{}", metric.name);
                    }
                    if !metric.unresolved_dependencies.is_empty() {
                        log::warn!(
                            "{family_code}/{arch_code}/{} has unresolved dependencies: {:?}",
                            metric.name,
                            metric.unresolved_dependencies
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

/// Atomically write the JSON document to `output`.
fn write_json(data: &Output, output: &Path) -> anyhow::Result<()> {
    let parent = match output.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    fs::create_dir_all(parent).with_context(|| format!("could not create {}", parent.display()))?;
    let mut tmp = tempfile::Builder::new()
        .prefix(&format!(".{}.", file_name(output)))
        .tempfile_in(parent)?;
    serde_json::to_writer_pretty(&mut tmp, data)?;
    tmp.write_all(b"\n")?;
    tmp.as_file().sync_all()?;
    tmp.persist(output)
        .with_context(|| format!("could not write {}", output.display()))?;
    Ok(())
}

fn run(args: &Args) -> anyhow::Result<()> {
    log::info!("Downloading LIKWID source...");
    let checkout = download_likwid_source(&args.repo, &args.branch, args.keep_workdir)?;
    let directories = sorted_entries(&checkout.root.join("groups"), true)?;
    log::info!("Found {} architecture directories.", directories.len());
    let mappings = parse_likwid_architecture_mapping(&checkout.root)?;
    let event_tables = parse_likwid_event_tables(&checkout.root)?;
    let data = Output {
        source: SourceInfo {
            repository: args.repo.clone(),
            groups_path: "groups".to_owned(),
            branch: args.branch.clone(),
            commit: checkout.commit.clone(),
            generated_at: Utc::now().to_rfc3339_opts(SecondsFormat::Micros, false),
        },
        architectures: build_architecture_records(&checkout.root, &mappings, &event_tables)?,
    };
    validate_output(&data)?;
    log::info!("Writing {}...", args.output.display());
    write_json(&data, &args.output)?;
    log::info!("Done.");
    if let Some(workdir) = &checkout.kept_workdir {
        log::info!("Source checkout kept at {}", workdir.display());
    }
    Ok(())
}

fn main() -> ExitCode {
    let args = Args::parse();
    env_logger::Builder::new()
        .filter_level(if args.verbose {
            log::LevelFilter::Debug
        } else {
            log::LevelFilter::Info
        })
        .format(|buf, record| writeln!(buf, "{}: {}", record.level(), record.args()))
        .init();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            log::error!("{e:#}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
#[path = "../../tests/bin/likwid_extractor.rs"]
mod tests;
