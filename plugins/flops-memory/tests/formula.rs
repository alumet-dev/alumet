use super::*;

#[test]
fn evaluates_arithmetic_and_counter_references() {
    let variables = HashMap::from([("PMC0", 10.0), ("PMC1", 4.0)]);
    assert_eq!(evaluate("(PMC0 + PMC1) * 2", &variables).unwrap(), 28.0);
}

#[test]
fn supports_c_conditionals_and_boolean_operators() {
    let variables = HashMap::from([("COUNT", 0.0)]);
    assert_eq!(evaluate("COUNT ? 3 : (2 + 5)", &variables).unwrap(), 7.0);
    assert_eq!(evaluate("COUNT == 0 || COUNT > 2", &variables).unwrap(), 1.0);
}

#[test]
fn supports_math_functions_and_hex_constants() {
    let variables = HashMap::from([("COUNT", 4.0)]);
    assert_eq!(evaluate("max(COUNT, 0x2) + sqrt(4)", &variables).unwrap(), 6.0);
}

#[test]
fn supports_common_c_numeric_casts() {
    let variables = HashMap::from([("COUNT", 4.8)]);
    assert_eq!(evaluate("(double)COUNT + (int)COUNT", &variables).unwrap(), 8.8);
}

#[test]
fn rejects_unknown_counters_and_division_by_zero_results() {
    let variables = HashMap::new();
    assert!(evaluate("MISSING + 1", &variables).is_err());
    assert!(evaluate("1 / 0", &variables).is_err());
}
