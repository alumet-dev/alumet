use lm_sensors::{
    FeatureRef,
    LMSensors,
    SubFeatureRef,
};

use alumet::{resources::{Resource}};

pub struct SensorsFeature<'a> {
    //coretemp_id: u32,
    resource: Resource,
    input_temperature_subfeature: SubFeatureRef<'a>, // Consider using SubFeature directly
}

impl SensorsFeature<'_> {
    pub fn new(lm_feature: FeatureRef, coretemp_id: u32) -> anyhow::Result<SensorsFeature> {
        let feature_label = lm_feature.label().expect("Component name is not a valid UTF-8 string.");
        let resource = resource_from_string(&feature_label, coretemp_id)?; //TODO handle failure
        let subfeature = lm_feature.sub_feature_by_kind(lm_sensors::value::Kind::TemperatureInput)?; //TODO handle failure

        Ok(SensorsFeature {
            //coretemp_id,
            resource,
            input_temperature_subfeature: subfeature
        })
    }

    pub fn read_temperature_value(&self) -> anyhow::Result<f64> {
        Ok(self.input_temperature_subfeature.raw_value()?)
    }

    pub fn get_resource(&self) -> Resource {
        self.resource.clone()
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

    // If we reach this line we could not parse the feature name,
    // so fallback to generic Resource
    // TODO: Raise an error?
    Ok(Resource::LocalMachine)
}

pub fn get_coretemp_feature_list<'a>(lmsensors: &'a LMSensors) -> Vec<SensorsFeature<'a>> {
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
                                   .expect("Unusual coretemp chip name")
                                   .parse()
                                   .expect("Coretemp chip name should be suffied by the coretemp id.");

        sensors_feature_list.extend(
            chip.feature_iter()
                // Filter by feature::Kind::Temperature just to be sure
                .filter(|x| x.kind() == Some(lm_sensors::feature::Kind::Temperature))
                .map(|x| SensorsFeature::new(x, coretemp_id)
                                         .expect("Could not create LMSensors feature."))
        );
    }

    sensors_feature_list
}
