use std::{string, sync::{Arc, Mutex}};

use alumet::{measurement::{AttributeValue, MeasurementPoint}, pipeline::Transform, resources::Resource};

pub struct EnergyAttributionTransform {
    pub metrics: Arc<Mutex<super::Metrics>>,
    pub buffer_pod: Vec<MeasurementPoint>,
    pub buffer_rapl: Vec<MeasurementPoint>,
    pub buffer_sched_pol: Vec<MeasurementPoint>,
}

impl Transform for EnergyAttributionTransform {
    fn apply(
        &mut self,
        measurements: &mut alumet::measurement::MeasurementBuffer,
    ) -> Result<(), alumet::pipeline::TransformError> {
        let metrics = self.metrics.lock().unwrap();

        let pod_id = metrics.cpu_usage_per_pod.unwrap();
        let rapl_id = metrics.rapl_consumed_energy.unwrap().as_u64();
        
        let besteffort = "besteffort".to_string();
        let burstable = "burstable".to_string();

        for m in measurements.clone().iter() {
            if m.metric.as_u64() == rapl_id {
                match m.resource {
                    Resource::CpuPackage{ id: _ }=> {
                        self.buffer_rapl.push(m.clone());
                    },
                    _ => continue,
                }
            } else if m.metric.as_u64() == pod_id.as_u64() {
                for (_, value) in m.attributes() {
                    if value.to_string() != besteffort && value.to_string() != burstable {
                            self.buffer_pod.push(m.clone());
                    }
                }
            }
        }
        println!("pod_id value: {pod_id:?}");
        println!("rapl_id value: {rapl_id:?}");


        // clear the buffer
        measurements.clear();


        // fill it again

        for m in self.buffer_pod.iter() {
            measurements.push(m.clone());
        }
        
        for m in self.buffer_rapl.iter() {
            measurements.push(m.clone());
        }

        self.buffer_pod.clear();
        self.buffer_rapl.clear();

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
