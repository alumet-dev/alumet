use anyhow::{anyhow};

use lm_sensors::{
    FeatureRef,
    LMSensors,
    SubFeatureRef,
};

use alumet::{resources::{Resource}};

pub struct SensorsFeature<'a> {
    pub label: String,
    pub resource: Resource,
    input_temperature_subfeature: SubFeatureRef<'a>, // Consider using SubFeature directly
}

impl SensorsFeature<'_> {
    pub fn new(lm_feature: FeatureRef, coretemp_id: u32) -> anyhow::Result<SensorsFeature> {
        let label = lm_feature.label().expect("Sensor feature name is not a valid UTF-8 string.");
        let resource = resource_from_string(&label, coretemp_id)?; // Error handled by the caller
        let subfeature = lm_feature.sub_feature_by_kind(lm_sensors::value::Kind::TemperatureInput)?; // Error handled by the caller

        Ok(SensorsFeature {
            label,
            resource,
            input_temperature_subfeature: subfeature
        })
    }

    pub fn read_temperature_value(&self) -> anyhow::Result<f64> {
        Ok(self.input_temperature_subfeature.raw_value()?)
    }
}

fn resource_from_string(name: &String, coretemp_id: u32) -> anyhow::Result<Resource> {
    let v: Vec<&str> = name.split_whitespace().collect();

    match v[0] {
        "Package" => {
            if let Ok(id) = v.get(2).expect("Expected Package id value").parse() {
                return Ok(Resource::CpuPackage {id});
            }
        },
        "Core" => {
            if let Ok(id) = v.get(1).expect("Expected Core id value").parse::<u32>() {
                // Need to attach the package id to the CPU core id
                let custom_id = format!("{}_{}", coretemp_id, id);
                return Ok(Resource::Custom{kind: std::borrow::Cow::Borrowed("CpuCore"), id: custom_id.into()})
            }
        },
        _ => {}
    }

    // If we reach this line we could not parse the feature name
    Err(anyhow!("Failed to parse the sensor feature name"))
}

pub fn get_coretemp_feature_list<'a>(lmsensors: &'a LMSensors, package_only: bool) -> Vec<SensorsFeature<'a>> {
    let mut sensors_feature_list: Vec<SensorsFeature> = vec![];
    for chip in lmsensors.chip_iter(None)
                         .filter(|x| x.name()
                                      .expect("Chip name from LMSensors is not a valid UTF-8 string.")
                                      .starts_with("coretemp")) {
        // We use filter and not find because there may exist multiple coretemp chips with the same name
        // (when there are two CPU packages for example)

        // The chip name is expected to be of the form 'coretemp-isa-0000'
        // So we retrieve the coretemp id from the suffix
        let coretemp_id: u32 = chip.name()
                                   .unwrap() // Unwrap is safe because of the expect call few lines above
                                   .split("-")
                                   .last()
                                   .expect("Coretemp chip name expected to be of the form 'coretemp-isa-XXXX'.")
                                   .parse()
                                   .expect("Coretemp chip name should be suffixed by the coretemp id.");

        sensors_feature_list.extend(
            chip.feature_iter()
                // Filter by feature::Kind::Temperature just to be sure
                .filter(|x| x.kind() == Some(lm_sensors::feature::Kind::Temperature))
                // If package_only is set, keep the feature if it corresponds to a Package temperature
                .filter(|x| !package_only || x.label().expect("Component name is not a valid UTF-8 string.").starts_with("Package"))
                .map(|x| SensorsFeature::new(x, coretemp_id)
                                         .expect("Could not create LMSensors feature."))
        );
    }

    sensors_feature_list
}
