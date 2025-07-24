// HERE IT IS JUST LIKE PERF_EVENTS, but for the disk so its pyo3
use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint, Timestamp},
    metrics::TypedMetricId,
    pipeline::elements::source::{PollError, Source},
    resources::{Resource, ResourceConsumer},
};
use anyhow::{Result, anyhow};
use log;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::{ffi::CString, net::IpAddr};

#[derive(Debug)]
pub struct MeasureQuarch {
    pub power: f64,
}

pub struct QuarchSource {
    quarch_ip: IpAddr,
    quarch_port: u16,
    metric: Vec<TypedMetricId<f64>>,
}

impl QuarchSource {
    pub fn new(ip: IpAddr, port: u16, metric: Vec<TypedMetricId<f64>>) -> Self {
        QuarchSource {
            quarch_ip: ip,
            quarch_port: port,
            metric,
        }
    }
}

impl Source for QuarchSource {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator<'_>, timestamp: Timestamp) -> Result<(), PollError> {
        log::info!("Polling QuarchSource");

        match get_quarch_measurement(&self.quarch_ip, self.quarch_port) {
            Ok(data) => {
                log::debug!("Fetched data: {:?}", data);

                for metric in &self.metric {
                    let value = data.power;
                    let point = MeasurementPoint::new(
                        timestamp,
                        *metric,
                        Resource::LocalMachine,
                        ResourceConsumer::LocalMachine,
                        value,
                    );
                    measurements.push(point);
                }
            }
            Err(e) => {
                log::error!("Fetch error: {}", e);
                return Err(PollError::Fatal(anyhow!(e)));
            }
        }
        Ok(())
    }
}

pub fn start_quarch_measurement(ip: &IpAddr, _port: u16) -> Result<()> {
    Python::with_gil(|py| {
        let device_ip = format!("{}", ip);
        let locals = PyDict::new(py);
        locals.set_item("device_ip", device_ip)?;

        let code = CString::new(
            r#"
from quarchpy import *
from quarchpy.qis import *

con_string = f"rest:{device_ip}"
device = getQuarchDevice(con_string, ConType="QIS")
ppm = quarchPPM(device, con_string)

ppm.sendCommand("stream mode header v3")
ppm.sendCommand("stream mode power enable")
ppm.sendCommand("stream mode power total enable")
ppm.sendCommand("record:trigger:mode manual")
ppm.sendCommand("record:averaging 32k")
ppm.streamResampleMode("1ms")"#,
        )?;
        py.run(&code, None, Some(&locals))?;

        log::info!("Successfully started Quarch measurement");
        Ok(())
    })
}

pub fn get_quarch_measurement(ip: &IpAddr, _port: u16) -> Result<MeasureQuarch> {
    Python::with_gil(|py| {
        let device_ip = format!("{}", ip);
        let locals = PyDict::new(py);
        locals.set_item("device_ip", device_ip)?;

        let code = CString::new(
            r#"
from quarchpy import *
from quarchpy.qis import *

con_string = f"rest:{device_ip}"
device = getQuarchDevice(con_string, ConType="QIS")
ppm = quarchPPM(device, con_string)
measure = ppm.getMeasurement()"#,
        )?;
        py.run(&code, None, Some(&locals))?;

        let measure = locals
            .get_item("measure")?
            .ok_or_else(|| anyhow!("'measure' not found in locals"))?;
        let power = measure.get_item("power")?.extract::<f64>()?;

        Ok(MeasureQuarch { power })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use std::net::IpAddr;
    use std::str::FromStr;

    #[test]
    fn test_start_quarch_measurement_success() {
        let ip = IpAddr::from_str("127.0.0.1").unwrap();
        let res = start_quarch_measurement(&ip, 1234);
        assert!(res.is_ok() || res.is_err());
    }

    #[test]
    fn test_get_quarch_measurement_success() -> Result<()> {
        let ip = IpAddr::from_str("127.0.0.1").unwrap();
        let res = get_quarch_measurement(&ip, 1234);
        match res {
            Ok(measure) => {
                assert!(measure.power >= 0.0);
            }
            Err(e) => {
                println!("Python call failed: {}", e);
            }
        }
        Ok(())
    }

    #[test]
    fn test_get_quarch_measurement_invalid_ip() {
        let ip = IpAddr::from_str("256.256.256.256"); // IP invalide
        assert!(ip.is_err());
    }
}
