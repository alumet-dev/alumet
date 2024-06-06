use alumet::{
    measurement::{MeasurementBuffer, MeasurementPoint, WrappedMeasurementValue},
    pipeline::{Transform, TransformError},
    plugin::{
        rust::{serialize_config, AlumetPlugin},
        ConfigTable,
    },
};

use serde::{Deserialize, Serialize};

pub struct TestTransform;
// {
//     config: Config,
// }


impl AlumetPlugin for TestTransform {
    fn name() -> &'static str {
        "units"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        Ok(Some(serialize_config(Config::default())?))
    }

    fn init(_: ConfigTable) -> anyhow::Result<Box<Self>> {
        Ok(Box::new(TestTransform))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetStart) -> anyhow::Result<()> {
        let transform = Box::new(TestTransform);
        alumet.add_transform(transform);
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

impl Transform for TestTransform {
    fn apply(&mut self, measurements: &mut MeasurementBuffer) -> Result<(), TransformError> {
        fn copy_and_change_to_float(m: &MeasurementPoint) -> MeasurementPoint {
            let mut res = m.clone();
            res.value = match res.value {
                f @ WrappedMeasurementValue::F64(_) => WrappedMeasurementValue::F64(1.0 as f64),
                WrappedMeasurementValue::U64(i) => WrappedMeasurementValue::F64(2.0 as f64),
            };
            res
        }
        // let copy: Vec<_> = measurements.iter().map(copy_and_change_to_float).collect();
        for value in &mut measurements.iter_mut() {
            *value = copy_and_change_to_float(value);
        }

        Ok(())
    }
}


#[derive(Deserialize, Serialize)]
struct Config {
    empty: String,
}

impl Default for Config {
    fn default() -> Self {
        Self{
            empty: String::new(),
        }
    }
}






#[cfg(test)]
mod tests {
    // use super::*;

    #[test]
    fn it_works() {
        assert_eq!(2+2, 4);
    }
}
