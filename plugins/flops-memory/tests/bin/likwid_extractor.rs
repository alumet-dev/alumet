//! Offline fixtures; run with `cargo test -p flops-memory --bin likwid_extractor`.

use super::*;

fn source_fixture() -> anyhow::Result<tempfile::TempDir> {
    let source = tempfile::tempdir()?;
    fs::create_dir_all(source.path().join("src/includes"))?;
    fs::write(
        source.path().join("src/includes/topology.h"),
        "#define TEST_FAMILY 6\n#define TEST_MODEL 0x42U\n#define SHARED_MODEL 0x43U\n\
                 #define ARM_TEST_FAMILY 8\n#define TEST_VENDOR 1\n#define TEST_PART 0x123U\n\
                 #define WHOLE_FAMILY 15\n#define WHOLE_MODEL 2\n",
    )?;
    fs::write(
        source.path().join("src/topology.c"),
        r#"
static const char *display = "Test processor";
static const char *early = "EARLY";
static const char *late = "LATE";
static const char *shared = "SHARED";
static const char *arm = "ARM";
static const char *whole = "WHOLE";
int topology_setName(void)
{
    switch (cpuid_info.family) {
    case TEST_FAMILY:
        switch (cpuid_info.model) {
        case TEST_MODEL:
            if (cpuid_info.stepping <= 4) {
                cpuid_info.name = display;
                cpuid_info.short_name = early;
            } else {
                cpuid_info.name = display;
                cpuid_info.short_name = late;
            }
            break;
        case SHARED_MODEL:
            cpuid_info.name = display;
            cpuid_info.short_name = shared;
            break;
        }
        break;
    case ARM_TEST_FAMILY:
        switch (cpuid_info.vendor) {
        case TEST_VENDOR:
            switch (cpuid_info.part) {
            case TEST_PART:
                cpuid_info.name = display;
                cpuid_info.short_name = arm;
                break;
            }
            break;
        }
        break;
    case WHOLE_FAMILY:
        switch (cpuid_info.model) {
        case WHOLE_MODEL:
            cpuid_info.name = display;
            break;
        }
        cpuid_info.short_name = whole;
        break;
    }
}
int topology_next(void) {}
"#,
    )?;
    fs::write(
        source.path().join("src/perfmon.c"),
        r#"
int
perfmon_init_maps(void)
{
    switch (cpuid_info.family) {
        case TEST_FAMILY:
            switch (cpuid_info.model) {
                // case SHARED_MODEL: eventHash = obsolete_arch_events;
                case TEST_MODEL:
                        if (cpuid_info.stepping <= 4) {
                            eventHash = first_arch_events;
                        } else {
                            eventHash = second_arch_events;
                        }
                        break;
                /* case SHARED_MODEL:
                        eventHash = obsolete_arch_events;
                        break; */
                case SHARED_MODEL:
                        eventHash = first_arch_events;
                        break;
            }
            break;
        case ARM_TEST_FAMILY:
            switch (cpuid_info.vendor) {
                case TEST_VENDOR:
                        switch (cpuid_info.part) {
                            case TEST_PART:
                                eventHash = arm_table_arch_events;
                                break;
                        }
                        break;
            }
            break;
        case WHOLE_FAMILY:
            eventHash = family_table_arch_events;
            break;
    }
}
"#,
    )?;
    for name in ["first", "second", "arm_table", "family_table"] {
        fs::write(
            source.path().join(format!("src/includes/perfmon_{name}_events.txt")),
            "EVENT_OPERATIONS 0xC7\nUMASK_OPERATIONS_ALL 0x01\n",
        )?;
    }
    Ok(source)
}

#[test]
fn derives_tables_from_topology_models_and_perfmon_selectors() -> anyhow::Result<()> {
    let source = source_fixture()?;
    let mappings = parse_likwid_architecture_mapping(source.path())?;
    let tables = parse_likwid_event_tables(source.path())?;
    for (architecture, table) in [
        ("EARLY", "first"),
        ("LATE", "second"),
        ("SHARED", "first"),
        ("ARM", "arm_table"),
        ("WHOLE", "family_table"),
    ] {
        let path = find_event_table(source.path(), architecture, mappings.get(architecture), &tables)?
            .expect("source-derived table should exist");
        assert_eq!(file_name(&path), format!("perfmon_{table}_events.txt"));
        assert_eq!(
            resolve_raw_counter("OPERATIONS_ALL", &parse_event_table(&path)?),
            "r01c7"
        );
    }
    let model = mappings["EARLY"]
        .model_definitions
        .iter()
        .find(|model| model.model_name == "TEST_MODEL")
        .expect("topology model should be preserved");
    assert_eq!(model.family, Some(6));
    assert_eq!(model.model_code, 0x42);
    assert_eq!(model.condition.as_deref(), Some("stepping <= 4"));
    assert_eq!(
        mappings["LATE"].model_definitions[0].condition.as_deref(),
        Some("stepping > 4")
    );
    assert_eq!(mappings["ARM"].model_definitions[0].family, Some(8));
    assert_eq!(mappings["EARLY"].architecture_names, ["Test processor"]);
    Ok(())
}

#[test]
fn stepping_comparisons_have_exact_complements() {
    for (operator, inverse) in [("<", ">="), ("<=", ">"), (">", "<="), (">=", "<")] {
        assert_eq!(
            stepping_conditions(operator, "5"),
            (format!("stepping {operator} 5"), format!("stepping {inverse} 5"))
        );
    }
}

#[test]
fn ignores_commented_out_table_selections() -> anyhow::Result<()> {
    let source = source_fixture()?;
    let tables = parse_likwid_event_tables(source.path())?;
    assert_eq!(tables.len(), 5);
    assert!(tables.iter().all(|table| table.table_name != "obsolete"));
    Ok(())
}

#[test]
fn does_not_guess_tables_for_unknown_architectures() -> anyhow::Result<()> {
    let source = source_fixture()?;
    let tables = parse_likwid_event_tables(source.path())?;
    fs::write(source.path().join("src/includes/perfmon_unknown_events.txt"), "")?;
    assert!(find_event_table(source.path(), "unknown", None, &tables)?.is_none());
    Ok(())
}

#[test]
fn missing_selected_table_is_unresolved() -> anyhow::Result<()> {
    let source = source_fixture()?;
    let mappings = parse_likwid_architecture_mapping(source.path())?;
    let tables = parse_likwid_event_tables(source.path())?;
    fs::remove_file(source.path().join("src/includes/perfmon_first_events.txt"))?;
    assert!(find_event_table(source.path(), "EARLY", mappings.get("EARLY"), &tables)?.is_none());
    Ok(())
}

#[test]
fn table_lookup_skips_unsupported_models_but_requires_matching_family_and_stepping() -> anyhow::Result<()> {
    let source = source_fixture()?;
    let mut mappings = parse_likwid_architecture_mapping(source.path())?;
    let tables = parse_likwid_event_tables(source.path())?;
    let mapping = mappings.get_mut("EARLY").unwrap();
    let mut supported = mapping
        .model_definitions
        .iter()
        .find(|model| model.model_name == "TEST_MODEL")
        .unwrap()
        .clone();
    let mut unsupported = supported.clone();
    unsupported.model_name = "UNSUPPORTED".to_owned();
    mapping.model_definitions = vec![unsupported, supported.clone()];
    assert!(find_event_table(source.path(), "EARLY", Some(mapping), &tables)?.is_some());

    supported.family_constant = Some("OTHER_FAMILY".to_owned());
    mapping.model_definitions = vec![supported.clone()];
    assert!(find_event_table(source.path(), "EARLY", Some(mapping), &tables)?.is_none());

    supported.family_constant = Some("TEST_FAMILY".to_owned());
    supported.condition = None;
    mapping.model_definitions = vec![supported];
    assert!(find_event_table(source.path(), "EARLY", Some(mapping), &tables)?.is_none());
    mapping.model_definitions.clear();
    assert!(find_event_table(source.path(), "EARLY", Some(mapping), &tables)?.is_none());
    Ok(())
}

#[test]
fn invalid_or_missing_topology_sources_are_errors() -> anyhow::Result<()> {
    let source = source_fixture()?;
    let topology = source.path().join("src/topology.c");
    fs::write(&topology, "int unrelated(void) {}\n")?;
    assert!(parse_likwid_architecture_mapping(source.path()).is_err());
    fs::remove_file(topology)?;
    assert!(parse_likwid_architecture_mapping(source.path()).is_err());
    Ok(())
}

#[test]
fn table_filenames_are_case_insensitive() -> anyhow::Result<()> {
    let source = source_fixture()?;
    let mappings = parse_likwid_architecture_mapping(source.path())?;
    let tables = parse_likwid_event_tables(source.path())?;
    fs::rename(
        source.path().join("src/includes/perfmon_first_events.txt"),
        source.path().join("src/includes/perfmon_First_events.txt"),
    )?;
    let path = find_event_table(source.path(), "EARLY", mappings.get("EARLY"), &tables)?
        .expect("case-insensitive filename should match");
    assert_eq!(file_name(&path), "perfmon_First_events.txt");
    Ok(())
}

#[test]
fn invalid_perfmon_source_is_an_error() -> anyhow::Result<()> {
    let source = source_fixture()?;
    let path = source.path().join("src/perfmon.c");
    for content in ["", "int perfmon_init_maps(void)\n{\n}\n"] {
        fs::write(&path, content)?;
        assert!(parse_likwid_event_tables(source.path()).is_err());
    }
    fs::remove_file(path)?;
    assert!(parse_likwid_event_tables(source.path()).is_err());
    Ok(())
}

fn event_table_fixture() -> anyhow::Result<EventTable> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("events.txt");
    fs::write(
        &path,
        "# comment\n\
             UMASK_ORPHAN 0x02\n\
             EVENT_OPERATIONS 0xC7 PMC\n\
             UMASK_OPERATIONS_ALL 0x0F # comment\n\
             UMASK_ALIAS 0x01\n\
             EVENT_MEMORY 0xAB\n\
             UMASK_MEMORY_READ 0x02\n\
             malformed\n\
             EVENT_INVALID 123\n",
    )?;
    parse_event_table(&path)
}

fn metrics_fixture() -> anyhow::Result<Output> {
    let source = source_fixture()?;
    let directory = source.path().join("groups/EARLY");
    fs::create_dir_all(&directory)?;
    fs::write(
        directory.join("FLOPS_DP.txt"),
        "EVENTSET\nPMC0 OPERATIONS_ALL\nMETRICS\nDP FLOPS [MFLOP/s] 1.0E-6*PMC0/time\n",
    )?;
    fs::write(
        directory.join("MEM1.txt"),
        "EVENTSET\nPMC1 OPERATIONS_ALL\nMETRICS\nMemory volume [GBytes] 1.0E-9*64*PMC1\n",
    )?;
    let mappings = parse_likwid_architecture_mapping(source.path())?;
    let tables = parse_likwid_event_tables(source.path())?;
    Ok(Output {
        source: SourceInfo {
            repository: "fixture".to_owned(),
            groups_path: "groups".to_owned(),
            branch: "test".to_owned(),
            commit: "test-commit".to_owned(),
            generated_at: "2026-01-01T00:00:00Z".to_owned(),
        },
        architectures: build_architecture_records(source.path(), &mappings, &tables)?,
    })
}

#[test]
fn parses_decimal_and_hexadecimal_integers() -> anyhow::Result<()> {
    for (input, expected) in [
        ("0", 0),
        ("42", 42),
        ("0x2a", 42),
        ("0X2A", 42),
        ("18446744073709551615", u64::MAX),
        ("0xffffffffffffffff", u64::MAX),
    ] {
        assert_eq!(parse_int(input)?, expected, "{input}");
    }
    Ok(())
}

#[test]
fn rejects_invalid_and_overflowing_integers() {
    for input in ["", "-1", "0x", "0xGG", "12U", " 12", "18446744073709551616"] {
        let error = parse_int(input).expect_err(input);
        assert!(error.to_string().contains("invalid integer constant"), "{input}");
    }
}

#[test]
fn recognizes_only_supported_group_filenames() {
    for name in ["FLOPS.txt", "FLOPS_DP.txt", "FLOPS_AVX512.txt"] {
        assert!(FLOPS_FILE_RE.is_match(name), "{name}");
    }
    for name in ["flops.txt", "FLOPS_.txt", "FLOPS_DP_extra.txt", "FLOPS.txt.bak"] {
        assert!(!FLOPS_FILE_RE.is_match(name), "{name}");
    }
    for name in ["MEM.txt", "MEM1.txt", "MEM12.txt"] {
        assert!(MEMORY_FILE_RE.is_match(name), "{name}");
    }
    for name in ["MEMORY.txt", "MEM_DP.txt", "mem.txt", "MEM1.txt.bak"] {
        assert!(!MEMORY_FILE_RE.is_match(name), "{name}");
    }
}

#[test]
fn parses_group_sections_with_comments_whitespace_and_crlf() {
    let parsed = parse_group_file(
        "# heading\r\nSHORT\r\nignored\r\n\r\n eventset \r\n\
             PMC0 OPERATIONS_ALL\r\nPMC1 MEMORY_READ\r\n---\r\n\
             metrics\r\nDP FLOPS [MFLOP/s] 1.E-6*PMC0/time\r\n\
             Memory volume [GBytes] 64*PMC1\r\n\
             LONG\r\nNot a metric [Bytes] PMC0\r\n",
    );
    assert_eq!(
        parsed.eventset,
        BTreeMap::from([
            ("PMC0".to_owned(), "OPERATIONS_ALL".to_owned()),
            ("PMC1".to_owned(), "MEMORY_READ".to_owned()),
        ])
    );
    assert_eq!(
        parsed.metrics,
        [
            ("DP FLOPS [MFLOP/s]".to_owned(), "1.E-6*PMC0/time".to_owned()),
            ("Memory volume [GBytes]".to_owned(), "64*PMC1".to_owned()),
        ]
    );
}

#[test]
fn ignores_empty_and_malformed_group_lines() {
    for input in ["", "# comment\n---\n", "PMC0 EVENT\nDP FLOPS [FLOP] PMC0"] {
        let parsed = parse_group_file(input);
        assert!(parsed.eventset.is_empty());
        assert!(parsed.metrics.is_empty());
    }
    let parsed = parse_group_file(
        "EVENTSET\nPMC0\nPMC1 EVENT EXTRA\nPMC2 VALID\n\
             METRICS\nmissing units PMC2\nMissing formula [FLOP]\nValid [FLOP] PMC2\n",
    );
    assert_eq!(parsed.eventset.len(), 1);
    assert_eq!(parsed.eventset["PMC2"], "VALID");
    assert_eq!(parsed.metrics, [("Valid [FLOP]".to_owned(), "PMC2".to_owned())]);
}

#[test]
fn formulas_documentation_is_not_parsed_as_metrics() {
    let parsed = parse_group_file("METRICS\nDP FLOPS [FLOP] PMC0\nFORMULAS: explanation\nExample [FLOP] PMC1\n");
    assert_eq!(parsed.metrics, [("DP FLOPS [FLOP]".to_owned(), "PMC0".to_owned())]);
}

#[test]
fn event_table_parsing_preserves_umask_parent_and_normalizes_hex() -> anyhow::Result<()> {
    let table = event_table_fixture()?;
    assert_eq!(table.events.len(), 2);
    assert_eq!(table.events["OPERATIONS"], "c7");
    assert_eq!(table.events["MEMORY"], "ab");
    assert_eq!(table.umasks["OPERATIONS_ALL"], "0f");
    assert_eq!(table.umask_events["ALIAS"], "c7");
    assert_eq!(table.umask_events["MEMORY_READ"], "ab");
    assert!(!table.umask_events.contains_key("ORPHAN"));
    assert!(!table.events.contains_key("INVALID"));
    Ok(())
}

#[test]
fn raw_counters_use_parent_events_and_ignore_event_options() -> anyhow::Result<()> {
    let table = event_table_fixture()?;
    for (input, expected) in [
        ("OPERATIONS_ALL", "r0fc7"),
        ("OPERATIONS_ALL:EDGEDETECT:THRESHOLD=2", "r0fc7"),
        ("ALIAS", "r01c7"),
        ("MEMORY_READ", "r02ab"),
        ("UNKNOWN", NOT_FOUND),
        ("OPERATIONS", NOT_FOUND),
        ("ORPHAN", NOT_FOUND),
        ("", NOT_FOUND),
    ] {
        assert_eq!(resolve_raw_counter(input, &table), expected, "{input}");
    }
    Ok(())
}

#[test]
fn raw_counter_fallback_uses_longest_complete_event_prefix() {
    let table = EventTable {
        events: HashMap::from([
            ("EVENT".to_owned(), "01".to_owned()),
            ("EVENT_LONG".to_owned(), "02".to_owned()),
        ]),
        umasks: HashMap::from([
            ("EVENT_LONG_ALL".to_owned(), "ff".to_owned()),
            ("EVENTUAL".to_owned(), "03".to_owned()),
        ]),
        umask_events: HashMap::new(),
    };
    assert_eq!(resolve_raw_counter("EVENT_LONG_ALL", &table), "rff02");
    assert_eq!(resolve_raw_counter("EVENTUAL", &table), NOT_FOUND);
}

#[test]
fn missing_or_non_utf8_event_tables_are_errors() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("events.txt");
    assert!(parse_event_table(&path).is_err());
    fs::write(&path, [0xff, 0xfe])?;
    assert!(parse_event_table(&path).is_err());
    fs::write(&path, "")?;
    let table = parse_event_table(&path)?;
    assert!(table.events.is_empty());
    assert!(table.umasks.is_empty());
    Ok(())
}

#[test]
fn identifies_flops_in_names_or_formulas_case_insensitively() {
    assert!(is_flops_metric("DP FLOPS [MFLOP/s]", "PMC0"));
    assert!(is_flops_metric("DP [operations]", "retired_flops"));
    assert!(!is_flops_metric("Memory bandwidth [Bytes/s]", "PMC0/time"));
}

#[test]
fn memory_selection_requires_volume_and_excludes_other_metrics() {
    for name in [
        "Memory data volume [GBytes]",
        "DRAM volume [Bytes]",
        "memory volume [Bytes]",
    ] {
        assert!(is_memory_volume_metric(name, "64*PMC0"), "{name}");
    }
    assert!(is_memory_volume_metric("Volume [Bytes]", "DRAM*64"));
    for name in [
        "Memory bandwidth [GBytes/s]",
        "Cache data volume [Bytes]",
        "Memory cache volume [Bytes]",
        "Memory latency volume [Bytes]",
        "Memory miss volume [Bytes]",
        "Memory hit volume [Bytes]",
        "Memory CPI volume [Bytes]",
        "Volume [Bytes]",
    ] {
        assert!(!is_memory_volume_metric(name, "PMC0"), "{name}");
    }
}

#[test]
fn splits_only_trailing_nonempty_metric_units() -> anyhow::Result<()> {
    assert_eq!(
        split_metric_name("DP FLOPS   [MFLOP/s]")?,
        ("DP FLOPS".to_owned(), "MFLOP/s".to_owned())
    );
    for name in ["DP FLOPS", "DP FLOPS []", "DP FLOPS [FLOP] extra"] {
        assert!(split_metric_name(name).is_err(), "{name}");
    }
    Ok(())
}

#[test]
fn normalizes_scaling_time_and_assumptions_without_changing_weights() {
    for formula in [
        "1E-6*PMC0/time",
        "1.0e-06*PMC0/TIME",
        "1e-9*PMC0/time",
        "(64 bytes assumed) 1.00E-009*PMC0/time",
    ] {
        assert_eq!(normalize_formula(formula), "PMC0", "{formula}");
    }
    for formula in ["64*PMC0 + 2*PMC1", "1e-3*PMC0", "(PMC0 + PMC1)/2", "PMC0/clock"] {
        assert_eq!(normalize_formula(formula), formula);
    }
}

#[test]
fn dependency_resolution_uses_whole_counter_names_and_deduplicates() -> anyhow::Result<()> {
    let table = event_table_fixture()?;
    let eventset = BTreeMap::from([
        ("PMC0".to_owned(), "OPERATIONS_ALL".to_owned()),
        ("PMC1".to_owned(), "MEMORY_READ".to_owned()),
        ("PMC10".to_owned(), "ALIAS".to_owned()),
    ]);
    let (events, unresolved) =
        resolve_metric_dependencies("PMC10 + PMC10 + OPERATIONS_ALL + PMC0", &eventset, Some(&table));
    assert_eq!(
        events,
        BTreeMap::from([
            ("PMC0".to_owned(), "r0fc7".to_owned()),
            ("PMC10".to_owned(), "r01c7".to_owned()),
        ])
    );
    assert!(unresolved.is_empty());
    let (events, unresolved) = resolve_metric_dependencies("OTHER_PMC0 + PMC100", &eventset, Some(&table));
    assert!(events.is_empty());
    assert!(unresolved.is_empty());
    Ok(())
}

#[test]
fn event_name_dependencies_expand_to_all_matching_counters() -> anyhow::Result<()> {
    let table = event_table_fixture()?;
    let eventset = BTreeMap::from([
        ("PMC0".to_owned(), "OPERATIONS_ALL".to_owned()),
        ("PMC1".to_owned(), "OPERATIONS_ALL".to_owned()),
    ]);
    let (events, unresolved) = resolve_metric_dependencies("OPERATIONS_ALL", &eventset, Some(&table));
    assert_eq!(events.len(), 2);
    assert!(events.values().all(|raw| raw == "r0fc7"));
    assert!(unresolved.is_empty());
    Ok(())
}

#[test]
fn missing_dependencies_are_explicit_and_unreferenced_events_are_ignored() -> anyhow::Result<()> {
    let table = event_table_fixture()?;
    let eventset = BTreeMap::from([
        ("PMC0".to_owned(), "OPERATIONS_ALL".to_owned()),
        ("PMC1".to_owned(), "UNKNOWN".to_owned()),
        ("PMC2".to_owned(), "UNUSED".to_owned()),
    ]);
    let (events, unresolved) = resolve_metric_dependencies("PMC0 + PMC1", &eventset, Some(&table));
    assert_eq!(events["PMC0"], "r0fc7");
    assert_eq!(events["PMC1"], NOT_FOUND);
    assert_eq!(unresolved, ["UNKNOWN"]);
    assert!(!events.contains_key("PMC2"));
    let (events, unresolved) = resolve_metric_dependencies("PMC0", &eventset, None);
    assert_eq!(events["PMC0"], NOT_FOUND);
    assert_eq!(unresolved, ["OPERATIONS_ALL"]);
    let (events, unresolved) = resolve_metric_dependencies("64", &eventset, None);
    assert!(events.is_empty());
    assert!(unresolved.is_empty());
    Ok(())
}

#[test]
fn metric_selection_normalizes_units_and_serializes_optional_dependencies() -> anyhow::Result<()> {
    let table = event_table_fixture()?;
    let group = parse_group_file(
        "EVENTSET\nPMC0 OPERATIONS_ALL\nPMC1 UNKNOWN\nMETRICS\n\
             DP FLOPS [MFLOP/s] 1.0E-6*PMC0/time\n\
             Memory volume [GBytes] 1.0E-9*64*PMC1\n\
             Memory bandwidth [GBytes/s] PMC1/time\n",
    );
    let flops = select_metrics(&group, true, Some(&table))?;
    assert_eq!(flops.len(), 1);
    assert_eq!(
        serde_json::to_value(&flops[0])?,
        serde_json::json!({
            "name": "DP FLOPS", "units": "FLOP", "formula": "PMC0",
            "events": {"PMC0": "r0fc7"}
        })
    );
    let memory = select_metrics(&group, false, Some(&table))?;
    assert_eq!(memory.len(), 1);
    assert_eq!(
        serde_json::to_value(&memory[0])?,
        serde_json::json!({
            "name": "Memory volume", "units": "Bytes", "formula": "64*PMC1",
            "events": {"PMC1": "not found"}, "unresolved_dependencies": ["UNKNOWN"]
        })
    );
    Ok(())
}

#[test]
fn selected_metrics_require_units_but_unselected_metrics_do_not() -> anyhow::Result<()> {
    let group = ParsedGroup {
        eventset: BTreeMap::new(),
        metrics: vec![("FLOPS".to_owned(), "PMC0".to_owned())],
    };
    assert!(select_metrics(&group, true, None).is_err());
    assert!(select_metrics(&group, false, None)?.is_empty());
    Ok(())
}

#[test]
fn builds_and_validates_records_from_local_source_files() -> anyhow::Result<()> {
    let data = metrics_fixture()?;
    validate_output(&data)?;
    assert_eq!(data.architectures.len(), 1);
    let records = &data.architectures["6"];
    assert_eq!(records.len(), 1);
    let record = &records[0];
    assert_eq!(record.definitions.architecture_abbreviation, "EARLY");
    assert_eq!(
        record.definitions.architecture_full_name.as_deref(),
        Some("Test processor")
    );
    assert!(record.definitions.models.iter().any(|model| model.model_code == 0x42));
    assert_eq!(record.metrics.flops.len(), 1);
    assert_eq!(record.metrics.flops[0].formula, "PMC0");
    assert_eq!(record.metrics.flops[0].events["PMC0"], "r01c7");
    assert_eq!(record.metrics.memory_volume.len(), 1);
    assert_eq!(record.metrics.memory_volume[0].units, "Bytes");
    Ok(())
}

#[test]
fn skips_unknown_architectures_and_non_metric_group_files() -> anyhow::Result<()> {
    let source = source_fixture()?;
    for architecture in ["UNKNOWN", "EARLY"] {
        fs::create_dir_all(source.path().join("groups").join(architecture))?;
    }
    fs::write(
        source.path().join("groups/UNKNOWN/FLOPS.txt"),
        "METRICS\nDP FLOPS [FLOP] PMC0\n",
    )?;
    fs::write(
        source.path().join("groups/EARLY/OTHER.txt"),
        "METRICS\nDP FLOPS [FLOP] PMC0\n",
    )?;
    let mappings = parse_likwid_architecture_mapping(source.path())?;
    let tables = parse_likwid_event_tables(source.path())?;
    assert!(build_architecture_records(source.path(), &mappings, &tables)?.is_empty());
    Ok(())
}

#[test]
fn records_keep_metrics_when_the_selected_event_table_is_missing() -> anyhow::Result<()> {
    let source = source_fixture()?;
    let directory = source.path().join("groups/EARLY");
    fs::create_dir_all(&directory)?;
    fs::write(
        directory.join("FLOPS.txt"),
        "EVENTSET\nPMC0 OPERATIONS_ALL\nMETRICS\nDP FLOPS [FLOP] PMC0\n",
    )?;
    fs::remove_file(source.path().join("src/includes/perfmon_first_events.txt"))?;
    let mappings = parse_likwid_architecture_mapping(source.path())?;
    let tables = parse_likwid_event_tables(source.path())?;
    let records = build_architecture_records(source.path(), &mappings, &tables)?;
    let metric = &records["6"][0].metrics.flops[0];
    assert_eq!(metric.events["PMC0"], NOT_FOUND);
    assert_eq!(metric.unresolved_dependencies, ["OPERATIONS_ALL"]);
    Ok(())
}

#[test]
fn validation_rejects_empty_units_and_formulas_in_both_categories() -> anyhow::Result<()> {
    for memory in [false, true] {
        for empty_units in [false, true] {
            let mut data = metrics_fixture()?;
            let record = &mut data.architectures["6"][0];
            let metric = if memory {
                &mut record.metrics.memory_volume[0]
            } else {
                &mut record.metrics.flops[0]
            };
            let name = metric.name.clone();
            if empty_units {
                metric.units.clear();
            } else {
                metric.formula.clear();
            }
            let error = validate_output(&data).expect_err("invalid metric should fail");
            let field = if empty_units { "units" } else { "formula" };
            assert_eq!(error.to_string(), format!("Empty {field} in 6/EARLY/{name}"));
        }
    }
    Ok(())
}

#[test]
fn validation_allows_explicitly_unresolved_dependencies() -> anyhow::Result<()> {
    let mut data = metrics_fixture()?;
    data.architectures["6"][0].metrics.flops[0]
        .unresolved_dependencies
        .push("UNKNOWN".to_owned());
    validate_output(&data)?;
    data.architectures.clear();
    validate_output(&data)?;
    Ok(())
}

#[test]
fn json_writing_creates_parents_and_replaces_existing_output() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("nested/metrics.json");
    let mut data = metrics_fixture()?;
    write_json(&data, &path)?;
    let text = fs::read_to_string(&path)?;
    assert!(text.ends_with('\n'));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&text)?,
        serde_json::to_value(&data)?
    );
    data.source.commit = "replacement".to_owned();
    write_json(&data, &path)?;
    let actual: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path)?)?;
    assert_eq!(actual["source"]["commit"], "replacement");
    assert_eq!(fs::read_dir(path.parent().unwrap())?.count(), 1);
    Ok(())
}

#[test]
fn json_write_failure_preserves_existing_destination_and_cleans_tempfiles() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let destination = directory.path().join("destination");
    fs::create_dir(&destination)?;
    fs::write(destination.join("keep.txt"), "preserve me")?;
    let data = metrics_fixture()?;
    assert!(write_json(&data, &destination).is_err());
    assert_eq!(fs::read_to_string(destination.join("keep.txt"))?, "preserve me");
    assert_eq!(fs::read_dir(directory.path())?.count(), 1);
    let blocked_parent = directory.path().join("file");
    fs::write(&blocked_parent, "not a directory")?;
    assert!(write_json(&data, &blocked_parent.join("metrics.json")).is_err());
    assert_eq!(fs::read_to_string(blocked_parent)?, "not a directory");
    Ok(())
}
