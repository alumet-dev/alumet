use super::*;

#[test]
fn bundled_likwid_metrics_are_valid() {
    let document = read_metrics_document(METRICS_JSON).unwrap();
    assert!(!document.architectures.is_empty());
}

#[test]
fn bundled_amd_extended_event_codes_parse_as_perf_counters() {
    let document = read_metrics_document(METRICS_JSON).unwrap();
    let x87 = document
        .architectures
        .values()
        .flatten()
        .flat_map(|record| &record.metrics.flops)
        .find(|metric| metric.name == "X87")
        .expect("bundled LIKWID metrics include the AMD X87 metric");

    assert_eq!(x87.events.get("PMC0").map(String::as_str), Some("r1000001c0"));
    let plan = CounterPlan::new(std::slice::from_ref(x87)).unwrap();
    assert_eq!(plan.counters.len(), 3);
}

#[test]
fn plans_every_flops_event_and_evaluates_all_metrics_from_the_complete_readout() {
    let document: MetricsDocument = serde_json::from_str(
        r#"{
            "architectures": {
                "6": [{
                    "definitions": {"models": [{"model_code": 42}]},
                    "metrics": {
                        "flops": [
                            {
                                "name": "All FLOPS",
                                "units": "FLOP",
                                "formula": "PMC0+PMC1+PMC2+PMC3+PMC4+PMC5+PMC6+PMC7+PMC8+PMC9",
                                "events": {
                                    "PMC0": "r01c7", "PMC1": "r02c7", "PMC2": "r03c7",
                                    "PMC3": "r04c7", "PMC4": "r05c7", "PMC5": "r06c7",
                                    "PMC6": "r07c7", "PMC7": "r08c7", "PMC8": "r09c7",
                                    "PMC9": "r0ac7"
                                }
                            },
                            {
                                "name": "Reused counter names FLOPS",
                                "units": "FLOP",
                                "formula": "2*PMC0 + 4*PMC1",
                                "events": {"PMC0": "r0ac7", "PMC1": "r01c7"}
                            },
                            {
                                "name": "Aliased FLOPS",
                                "units": "FLOP",
                                "formula": "PMC0 + PMC1",
                                "events": {
                                    "PMC0": "r02c7", "PMC1": "r02c7",
                                    "UNUSED_IN_FORMULA": "r0bc7"
                                }
                            }
                        ],
                        "memory_volume": [{
                            "name": "Not a FLOPS metric",
                            "units": "Bytes",
                            "formula": "PMC0",
                            "events": {"PMC0": "not found"}
                        }]
                    }
                }]
            }
        }"#,
    )
    .unwrap();
    let cpu = CpuInfo {
        family: 6,
        model: 42,
        stepping: 0,
    };
    let record = select_architecture(&document, &cpu).unwrap();
    let plan = CounterPlan::new(&record.metrics.flops).unwrap();
    let names: Vec<_> = plan.counters.iter().map(ParsedEvent::name).collect();
    assert_eq!(
        names,
        [
            "r01c7", "r02c7", "r03c7", "r04c7", "r05c7", "r06c7", "r07c7", "r08c7", "r09c7", "r0ac7", "r0bc7",
        ]
    );
    assert_eq!(plan.metrics.len(), 3);

    for scale in [1, 2] {
        let values: Vec<_> = (1..=11).map(|value| value * scale).collect();
        let results: Vec<_> = plan
            .metrics
            .iter()
            .map(|metric| evaluate_metric(&metric.definition.formula, &metric.inputs, &values).unwrap())
            .collect();
        assert_eq!(results, [55.0, 24.0, 4.0].map(|value| value * scale as f64));
    }
}

#[test]
fn missing_counter_values_are_errors_not_partial_metrics() {
    let inputs = vec![("PMC0".to_owned(), 0), ("PMC9".to_owned(), 9)];
    let error = evaluate_metric("PMC0 + PMC9", &inputs, &[1]).unwrap_err();
    assert!(error.to_string().contains("missing raw counter value for 'PMC9'"));
}

#[test]
fn metric_accuracy_is_the_least_accurate_input() {
    let inputs = vec![("PMC0".to_owned(), 0), ("PMC1".to_owned(), 1)];
    assert_eq!(
        metric_accuracy(&inputs, &[Accuracy::Exact, Accuracy::Extrapolated]).unwrap(),
        Accuracy::Extrapolated
    );
    assert_eq!(
        metric_accuracy(&inputs, &[Accuracy::Underestimated, Accuracy::Extrapolated]).unwrap(),
        Accuracy::Underestimated
    );
    assert!(metric_accuracy(&inputs, &[Accuracy::Exact]).is_err());
}

#[test]
fn architecture_selection_matches_family_model_and_stepping() {
    let document: MetricsDocument = serde_json::from_str(
        r#"{
            "architectures": {
                "6": [{
                    "definitions": {"models": [
                        {"model_code": 42, "condition": "stepping <= 4"}
                    ]},
                    "metrics": {
                        "flops": [],
                        "memory_volume": [{
                            "name": "ignored memory metric",
                            "units": "Bytes",
                            "formula": "PMC0",
                            "events": {"PMC0": "not found"},
                            "unresolved_dependencies": ["missing event"]
                        }]
                    }
                }]
            }
        }"#,
    )
    .unwrap();
    let supported = CpuInfo {
        family: 6,
        model: 42,
        stepping: 3,
    };
    let record = select_architecture(&document, &supported).unwrap();
    assert!(record.metrics.flops.is_empty());
    let other_stepping = CpuInfo {
        stepping: 5,
        ..supported
    };
    assert!(select_architecture(&document, &other_stepping).is_none());
}

#[test]
fn parses_likwid_units() {
    assert_eq!(unit_from_likwid("Bytes").unwrap(), Unit::Byte);
    assert!(unit_from_likwid("cycles").is_err());
}

#[test]
fn evaluates_supported_model_stepping_conditions() {
    assert!(condition_matches(Some("stepping <= 4"), 4));
    assert!(!condition_matches(Some("stepping > 4"), 4));
    assert!(!condition_matches(Some("stepping > 4"), -1));
    assert!(!condition_matches(Some("vendor == 1"), 4));
}
