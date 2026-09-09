use anyhow::{anyhow, Context};

use lm_sensors::{FeatureRef, LMSensors, SubFeatureRef};

use alumet::resources::Resource;

pub struct CoretempSensor<'a> {
    pub label: String,
    pub resource: Resource,
    input_temperature_subfeature: SubFeatureRef<'a>,
}

impl CoretempSensor<'_> {
    pub fn new(lm_feature: FeatureRef, coretemp_id: u32) -> anyhow::Result<CoretempSensor> {
        let label = lm_feature
            .label()
            .context("Coretemp sensor label is not a valid UTF-8 string")?;
        let resource = resource_from_string(&label, coretemp_id)?; // Error handled by the caller
        let subfeature = lm_feature.sub_feature_by_kind(lm_sensors::value::Kind::TemperatureInput)?; // Error handled by the caller

        Ok(CoretempSensor {
            label,
            resource,
            input_temperature_subfeature: subfeature,
        })
    }

    pub fn read_temperature_value(&self) -> anyhow::Result<f64> {
        Ok(self.input_temperature_subfeature.raw_value()?)
    }
}

fn resource_from_string(name: &str, coretemp_id: u32) -> anyhow::Result<Resource> {
    let v: Vec<&str> = name.split_whitespace().collect();

    match v[0] {
        "Package" => {
            if let Ok(id) = v.get(2).context("Failed to parse the coretemp sensor name {name}, expected a Package id value")?.parse::<u32>() {
                return Ok(Resource::CpuPackage { id });
            }
        }
        "Core" => {
            if let Ok(id) = v.get(1).context("Failed to parse the coretemp sensor name {name}, expected a Core id value")?.parse::<u32>() {
                // Need to attach the package id to the CPU core id
                let custom_id = format!("{}_{}", coretemp_id, id);
                return Ok(Resource::Custom {
                    kind: std::borrow::Cow::Borrowed("cpu_core"),
                    id: custom_id.into(),
                });
            }
        }
        _ => {}
    }

    // If we reach this line we could not parse the feature name
    Err(anyhow!("Failed to parse the coretemp sensor name {name}"))
}

pub fn get_coretemp_sensors_list<'a>(lmsensors: &'a LMSensors, package_only: bool) -> anyhow::Result<Vec<CoretempSensor<'a>>> {
    let mut coretemp_sensors_list: Vec<CoretempSensor> = vec![];
    for chip in lmsensors.chip_iter(None).filter(|x| {
        // Silently filter out chip name not in UTF-8 format by calling unwrap_or_default
        x.name()
            .unwrap_or_default()
            .starts_with("coretemp-isa-")
    }) {
        // We use filter and not find because there may exist multiple coretemp chips with the same name
        // (when there are two CPU packages for example)

        // The chip name is expected to be of the form 'coretemp-isa-0000'
        // So we retrieve the coretemp id from the suffix
        let coretemp_id: u32 = match chip
            .name()
            .unwrap() // Unwrap is safe because of the filter call few lines above
            .split('-')
            .last()
            .unwrap() // Unwrap is safe because of the filter call few lines above
            .parse() {
                Ok(id) => id,
                Err(_) => return Err(anyhow!("Unexpected coretemp name ({}) should be of the form 'coretemp-isa-XXXX' with 'XXXX' being the package id", chip.name().unwrap()))
            };

        let tmp_sensors_list: anyhow::Result<Vec<CoretempSensor<'a>>> = chip
            .feature_iter()
            // Filter by feature::Kind::Temperature just to be sure
            .filter(|x| x.kind() == Some(lm_sensors::feature::Kind::Temperature))
            // If package_only is set, keep the feature if it corresponds to a Package temperature
            .filter(|x| {
                !package_only
                    || x.label()
                        .map_or_else(|_| {true}, // Silently pass the error to be treated in CoretempSensor constructor
                                     |v| v.starts_with("Package"))
            })
            .map(|x| CoretempSensor::new(x, coretemp_id))
            // If CoretempSensor constructor fails, the Err is propagated in the collect
            .collect();

        match tmp_sensors_list {
            Ok(tmp_sensors_list) => coretemp_sensors_list.extend(tmp_sensors_list),
            Err(e) => return Err(anyhow!("Could not create coretemp sensor: {e}"))
        }
    }

    Ok(coretemp_sensors_list)
}
