use alumet::measurement::{MeasurementPoint, WrappedMeasurementValue};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone, Copy)]
pub(crate) enum Function {
    Sum,
    Mean,
}

impl Function {
    pub(crate) fn name(self) -> String {
        match self {
            Function::Sum => "sum".to_string(),
            Function::Mean => "mean".to_string(),
        }
    }

    pub(crate) fn function(self) -> fn(Vec<MeasurementPoint>) -> Option<WrappedMeasurementValue> {
        match self {
            Function::Sum => sum,
            Function::Mean => mean,
        }
    }
}

/// Returns the aggregated sum result of the given vec.
pub(crate) fn sum(sub_vec: Vec<MeasurementPoint>) -> Option<WrappedMeasurementValue> {
    sub_vec.iter().map(|x| x.clone().value).reduce(|x, y| match (x, y) {
        (WrappedMeasurementValue::F64(fx), WrappedMeasurementValue::F64(fy)) => WrappedMeasurementValue::F64(fx + fy),
        (WrappedMeasurementValue::U64(ux), WrappedMeasurementValue::U64(uy)) => WrappedMeasurementValue::U64(ux + uy),
        (_, _) => unreachable!("should not receive mixed U64 and F64 values"),
    })
}

/// Returns the aggregated mean result of the given vec.
pub(crate) fn mean(sub_vec: Vec<MeasurementPoint>) -> Option<WrappedMeasurementValue> {
    let Some(result) = sub_vec.iter().map(|x| x.clone().value).reduce(|x, y| match (x, y) {
        (WrappedMeasurementValue::F64(fx), WrappedMeasurementValue::F64(fy)) => WrappedMeasurementValue::F64(fx + fy),
        (WrappedMeasurementValue::U64(ux), WrappedMeasurementValue::U64(uy)) => WrappedMeasurementValue::U64(ux + uy),
        (_, _) => unreachable!("should not receive mixed U64 and F64 values"),
    }) else {
        return None;
    };

    Some(match result {
        WrappedMeasurementValue::F64(fx) => WrappedMeasurementValue::F64(fx / sub_vec.len() as f64),
        WrappedMeasurementValue::U64(ux) => WrappedMeasurementValue::U64(ux / sub_vec.len() as u64),
    })
}

#[cfg(test)]
mod tests {
    use crate::aggregations::Function;

    #[test]
    fn test_function_get_string() {
        assert_eq!(Function::Mean.name(), "mean");
        assert_eq!(Function::Sum.name(), "sum");
    }

    mod sum {
        use alumet::measurement::WrappedMeasurementValue;

        use crate::{
            aggregations::{sum, Function},
            transform::tests::new_point,
        };

        #[test]
        fn empty_vec() {
            assert_eq!(sum(vec![]), None)
        }

        #[test]
        fn u64_sub_vec() {
            let sub_vec = vec![
                new_point("2025-02-10T13:19:00Z", WrappedMeasurementValue::U64(0), 0),
                new_point("2025-02-10T13:19:00Z", WrappedMeasurementValue::U64(1), 0),
                new_point("2025-02-10T13:19:00Z", WrappedMeasurementValue::U64(3), 0),
                new_point("2025-02-10T13:19:00Z", WrappedMeasurementValue::U64(56), 0),
            ];

            let Some(WrappedMeasurementValue::U64(result)) = sum(sub_vec) else {
                panic!("not an u64")
            };

            assert_eq!(result, 60);
        }

        #[test]
        fn f64_sub_vec() {
            let sub_vec = vec![
                new_point("2025-02-10T13:19:00Z", WrappedMeasurementValue::F64(0.0), 0),
                new_point("2025-02-10T13:19:00Z", WrappedMeasurementValue::F64(1.5), 0),
                new_point("2025-02-10T13:19:00Z", WrappedMeasurementValue::F64(3.6), 0),
                new_point("2025-02-10T13:19:00Z", WrappedMeasurementValue::F64(56.9), 0),
            ];

            let Some(WrappedMeasurementValue::F64(result)) = sum(sub_vec) else {
                panic!("not an u64")
            };

            assert_eq!(result, 62 as f64);
        }

        #[test]
        #[should_panic]
        fn mixed_f64_and_u64() {
            let sub_vec = vec![
                new_point("2025-02-10T13:19:00Z", WrappedMeasurementValue::U64(0), 0),
                new_point("2025-02-10T13:19:00Z", WrappedMeasurementValue::F64(1.5), 0),
            ];

            Function::Sum.function()(sub_vec);
        }
    }

    mod mean {
        use alumet::measurement::WrappedMeasurementValue;

        use crate::{
            aggregations::{mean, Function},
            transform::tests::new_point,
        };

        #[test]
        fn empty_vec() {
            assert_eq!(mean(vec![]), None)
        }

        #[test]
        fn u64_sub_vec() {
            let sub_vec = vec![
                new_point("2025-02-10T13:19:00Z", WrappedMeasurementValue::U64(0), 0),
                new_point("2025-02-10T13:19:00Z", WrappedMeasurementValue::U64(1), 0),
                new_point("2025-02-10T13:19:00Z", WrappedMeasurementValue::U64(3), 0),
                new_point("2025-02-10T13:19:00Z", WrappedMeasurementValue::U64(56), 0),
            ];

            let Some(WrappedMeasurementValue::U64(result)) = mean(sub_vec) else {
                panic!("not an u64")
            };

            assert_eq!(result, 15);
        }

        #[test]
        fn f64_sub_vec() {
            let sub_vec = vec![
                new_point("2025-02-10T13:19:00Z", WrappedMeasurementValue::F64(0.5), 0),
                new_point("2025-02-10T13:19:00Z", WrappedMeasurementValue::F64(1.6), 0),
                new_point("2025-02-10T13:19:00Z", WrappedMeasurementValue::F64(3.0), 0),
                new_point("2025-02-10T13:19:00Z", WrappedMeasurementValue::F64(56.85), 0),
            ];

            let Some(WrappedMeasurementValue::F64(result)) = mean(sub_vec) else {
                panic!("not an u64")
            };

            assert_eq!(result, 15.4875);
        }

        #[test]
        #[should_panic]
        fn mixed_f64_and_u64() {
            let sub_vec = vec![
                new_point("2025-02-10T13:19:00Z", WrappedMeasurementValue::U64(0), 0),
                new_point("2025-02-10T13:19:00Z", WrappedMeasurementValue::F64(1.5), 0),
            ];

            Function::Mean.function()(sub_vec);
        }
    }
}
