use alumet::measurement::{MeasurementPoint, WrappedMeasurementValue};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone, Copy)]
pub(crate) enum Function {
    Sum,
    Mean,
}

impl Function {
    pub(crate) fn get_string(self) -> String {
        match self {
            Function::Sum => "sum".to_string(),
            Function::Mean => "mean".to_string(),
        }
    }

    pub(crate) fn get_function(self) -> fn(Vec<MeasurementPoint>) -> WrappedMeasurementValue {
        match self {
            Function::Sum => sum,
            Function::Mean => mean,
        }
    }
}

/// Returns the aggregated sum result of the given vec.
pub(crate) fn sum(sub_vec: Vec<MeasurementPoint>) -> WrappedMeasurementValue {
    let result = sub_vec
        .iter()
        .map(|x| x.clone().value)
        .reduce(|x, y| {
            match (x, y) {
                (WrappedMeasurementValue::F64(fx), WrappedMeasurementValue::F64(fy)) => {
                    WrappedMeasurementValue::F64(fx + fy)
                }
                (WrappedMeasurementValue::U64(ux), WrappedMeasurementValue::U64(uy)) => {
                    WrappedMeasurementValue::U64(ux + uy)
                }
                (_, _) => panic!("Pas normal"), // TODO Fix this panic line
            }
        })
        .unwrap();

    result
}

/// Returns the aggregated mean result of the given vec.
pub(crate) fn mean(sub_vec: Vec<MeasurementPoint>) -> WrappedMeasurementValue {
    let result = sub_vec
        .iter()
        .map(|x| x.clone().value)
        .reduce(|x, y| {
            match (x, y) {
                (WrappedMeasurementValue::F64(fx), WrappedMeasurementValue::F64(fy)) => {
                    WrappedMeasurementValue::F64(fx + fy)
                }
                (WrappedMeasurementValue::U64(ux), WrappedMeasurementValue::U64(uy)) => {
                    WrappedMeasurementValue::U64(ux + uy)
                }
                (_, _) => panic!("Pas normal"), // TODO Fix this panic line
            }
        })
        .unwrap();

    match result {
        WrappedMeasurementValue::F64(fx) => WrappedMeasurementValue::F64(fx / sub_vec.len() as f64),
        WrappedMeasurementValue::U64(ux) => WrappedMeasurementValue::U64(ux / sub_vec.len() as u64),
    }
}

#[cfg(test)]
mod tests {
    use crate::aggregations::Function;


    #[test]
    fn test_function_get_string() {
        assert_eq!(Function::Mean.get_string(), "mean");
        assert_eq!(Function::Sum.get_string(), "sum");
    }

//     #[test]
//     fn test_function_get_function() {
//         assert_eq!(Function::Mean.get_function(), mean);
//         assert_eq!(Function::Sum.get_string(), "sum");
//     }
}