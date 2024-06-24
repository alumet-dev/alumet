use std::{collections::HashMap, sync::{Arc, Mutex}};

use alumet::{measurement::{MeasurementPoint, WrappedMeasurementValue}, pipeline::Transform, resources::Resource};

pub struct EnergyAttributionTransform {
    pub metrics: Arc<Mutex<super::Metrics>>,
    buffer_pod: HashMap<u64, Vec<MeasurementPoint>>,
}

impl EnergyAttributionTransform {
    pub fn new(metrics: Arc<Mutex<super::Metrics>>) -> Self {
        Self { 
            metrics,
            buffer_pod: HashMap::<u64, Vec<MeasurementPoint>>::new()
        }
    }
}

impl Transform for EnergyAttributionTransform {
    fn apply(
        &mut self,
        measurements: &mut alumet::measurement::MeasurementBuffer,
    ) -> Result<(), alumet::pipeline::TransformError> {
        let metrics = self.metrics.lock().unwrap();

        let pod_id = metrics.cpu_usage_per_pod.unwrap();
        let rapl_id = metrics.rapl_consumed_energy.unwrap().as_u64();
        let metric_id = metrics.pod_attributed_energy.unwrap();
        
        let besteffort = "besteffort".to_string();
        let burstable = "burstable".to_string();

        for m in measurements.clone().iter() {
            if m.metric.as_u64() == rapl_id {
                match m.resource {
                    Resource::CpuPackage{ id: _ }=> {
                        // self.buffer_rapl.push(m.clone());

                        let id = m.timestamp.get_sec();

                        if !self.buffer_pod.contains_key(&id) {
                            continue;
                        }

                        let length = self.buffer_pod.get(&id).unwrap().len() as f64;
                        let mut new_m: MeasurementPoint;

                        for point in self.buffer_pod.remove(&id).unwrap().iter() {
                            new_m = MeasurementPoint::new(
                                m.timestamp, 
                                metric_id,
                                point.resource.clone(),
                                point.consumer.clone(),
                                match m.value {
                                    WrappedMeasurementValue::F64(fx) => fx / length,
                                    WrappedMeasurementValue::U64(ux) => (ux as f64) / length,
                                }
                            );

                            for (key, value) in point.clone().attributes() {
                                new_m = new_m.with_attr(format!("{key}"), value.clone());
                            }

                            measurements.push(new_m.clone());
                        }
                    },
                    _ => continue,
                }
            } else if m.metric.as_u64() == pod_id.as_u64() {
                for (_, value) in m.attributes() {
                    if value.to_string() != besteffort && value.to_string() != burstable {
                            // self.buffer_pod.push(m.clone());
                            let id = m.timestamp.get_sec();
                            match self.buffer_pod.get_mut(&id) {
                                Some(vec_points) => {
                                    vec_points.push(m.clone());
                                }
                                None => {
                                    self.buffer_pod.insert(id.clone(), vec![m.clone()]);
                                }
                            }
                    }
                }
            }
        }

        // on regarde temps ecoulé total CPU tc2-tc1 -> tcpu (buffer_pod.last - buffer_pod.first)
        // on regarde temps ecoulé total rapl tr2-tr1 -> trapl (buffer_rapl.last - buffer_rapl.first)
        // Pour chaque pod
        // pour chaque mesure du temps cpu on ajoute les temps passé par la mesure -> tottime_pod
        // pour chaque mesure rapl on ajoute les valeurs passées par la mesure -> totnrj
        // Fin Pour
        // Pour chaque pod 
        // valeur_conso = tottime_pod/tcpu * totnrj
        // Fin Pour
        // Pour chaque valeur_conso on push la valeur
        
        
        // todo!("attribution")

        Ok(())
    }
}
