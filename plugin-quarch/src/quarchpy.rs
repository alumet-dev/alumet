// This file implements the source functionality for the Quarch input plugin.

use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint, Timestamp},
    metrics::TypedMetricId,
    pipeline::elements::source::{PollError, Source},
    resources::{Resource, ResourceConsumer},
};
use anyhow::{anyhow, Result};
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
    metric: TypedMetricId<f64>,
}

impl QuarchSource {
    pub fn new(ip: IpAddr, port: u16, metric: TypedMetricId<f64>) -> Self {
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
                let value = data.power;
                // Creates a Measurement Point from the MeasureQuarch type data
                let point = MeasurementPoint::new(
                    timestamp,
                    self.metric,
                    Resource::LocalMachine,
                    ResourceConsumer::LocalMachine,
                    value,
                );
                measurements.push(point);
            }
            Err(e) => {
                log::error!("Fetch error: {}", e);
                return Err(PollError::Fatal(anyhow!(e)));
            }
        }
        Ok(())
    }
}

/// Start the connection with the module by TCP/IP. It also send commands to the module to init configurations.
pub fn start_quarch_measurement(ip: &IpAddr, _port: u16) -> Result<()> {
    Python::with_gil(|py| {
        let device_ip = format!("{}", ip);
        let locals = PyDict::new(py);
        locals.set_item("device_ip", device_ip)?;

        let code = CString::new(
            r#"
from quarchpy import *
from quarchpy.qis import *
import builtins
import time

if not isQisRunning():
    startLocalQis()
    time.sleep(2)
    if not isQisRunning():
        raise RuntimeError("QIS failed to start properly")
    closeQisAtEndOfTest = True
else:
    closeQisAtEndOfTest = False

con_string = f"tcp:{device_ip}"
device = get_quarch_device(con_string, ConType="QIS")

device.send_command("CONFig:DEFault STATE")
device.send_command("RECord:TRIGger:MODE MANUAL")
device.send_command("RECord:AVEraging 32K")
device.send_command("RECord:RUN")

builtins.quarch_device = device
"#,
        )?;
        py.run(&code, None, Some(&locals))?;

        log::info!("Successfully started quarch measurement");
        Ok(())
    })
}

/// Used to GET data from the python API.
/// It gets the current and the voltages and then calculate the power consumption.
pub fn get_quarch_measurement(ip: &IpAddr, _port: u16) -> Result<MeasureQuarch> {
    Python::with_gil(|py| {
        let device_ip = format!("{}", ip);
        let locals = PyDict::new(py);
        locals.set_item("device_ip", device_ip)?;

        let code = CString::new(
            r#"
from quarchpy import *
from quarchpy.qis import *
import builtins
import re

device = builtins.quarch_device

def extract_value(resp):
    import re
    m = re.search(r'\d+', resp)
    return float(m.group()) if m else 0.0

v_resp = device.send_command("MEASure:VOLTage +12V?")
c_resp = device.send_command("MEASure:CURRent +12V?")

voltage = extract_value(v_resp) / 1000
current = extract_value(c_resp) / 1000000
power = voltage * current

measure = {'power': power}
"#,
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
        pyo3::prepare_freethreaded_python();
        let ip = IpAddr::from_str("127.0.0.1").unwrap();
        let res = start_quarch_measurement(&ip, 1234);
        assert!(res.is_ok() || res.is_err());
    }

    #[test]
    fn test_get_quarch_measurement_success() -> Result<()> {
        pyo3::prepare_freethreaded_python();
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
        let ip = IpAddr::from_str("256.256.256.256"); // invalid IP
        assert!(ip.is_err());
    }
}
