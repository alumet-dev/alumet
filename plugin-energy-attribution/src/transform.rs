use std::sync::{Arc, Mutex};

use alumet::{measurement::MeasurementPoint, pipeline::Transform};

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
        // let metrics = self.metrics.lock().unwrap();

        // let pod_id = metrics.cpu_usage_per_pod.unwrap();
        // let rapl_id = metrics.rapl_consumed_energy.unwrap();
        
        // for m in measurements.clone().iter() {
        //     match m.metric {
        //         rapl_id => {
        //             self.buffer_rapl.push(m.clone());
        //         },
        //         pod_id => {
        //             self.buffer_pod.push(m.clone());
        //         },
        //         _ => {
        //             continue;
        //         }
        //     }
        // }
        // println!("pod_id value: {pod_id:?}");
        // println!("rapl_id value: {rapl_id:?}");

        // // on regarde temps ecoulé total CPU tc2-tc1 -> tcpu (buffer_pod.last - buffer_pod.first)
        // // on regarde temps ecoulé total rapl tr2-tr1 -> trapl (buffer_rapl.last - buffer_rapl.first)
        // // Pour chaque pod
        // // pour chaque mesure du temps cpu on ajoute les temps passé par la mesure -> tottime_pod
        // // pour chaque mesure rapl on ajoute les valeurs passées par la mesure -> totnrj
        // // Fin Pour
        // // Pour chaque pod 
        // // valeur_conso = tottime_pod/tcpu * totnrj
        // // Fin Pour
        // // Pour chaque valeur_conso on push la valeur
        
        
        // todo!("attribution")
        println!("Measurement length: {:?}", measurements.len());
        for m in measurements.clone().iter(){
            println!("{m:?}");
        }
        Ok(())
    }
}
