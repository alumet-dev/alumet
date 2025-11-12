// This file implements the source functionality for the Quarch input plugin.
use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint, Timestamp},
    metrics::TypedMetricId,
    pipeline::elements::source::{PollError, Source},
    resources::{Resource, ResourceConsumer},
};
use anyhow::{Context, Result, anyhow};
use log;
use std::{
    env,
    io::{Read, Write},
    net::{IpAddr, TcpStream},
    process::{Child, Command},
    sync::{Arc, Mutex},
    thread::sleep,
    time::{Duration, Instant, SystemTime},
};

#[derive(Debug)]
pub struct MeasureQuarch {
    pub power: f64,
    pub timestamp: Timestamp,
}

/// Manages the connection and interaction with a Quarch device for power measurements.
pub struct QuarchSource {
    quarch_ip: IpAddr,
    quarch_port: u16,
    sample: u32,
    metric: TypedMetricId<f64>,
    pub(crate) stream: Option<TcpStream>,
}

/// Wraps QuarchSource in a thread-safe `Arc<Mutex<...>>` to enable shared access.
///
/// Alumet’s pipeline requires sources to be boxed (Box<dyn Source>) and passed to it.
/// However, the Quarch plugin also needs to stop measurements and handle events (at the end of a measurement),
/// which requires access to the same QuarchSource instance from multiple places (pipeline, event handlers).
/// This wrapper ensures thread-safe, shared access to the source.
pub struct SourceWrapper {
    pub(crate) inner: Arc<Mutex<QuarchSource>>,
}

impl QuarchSource {
    ///  Initializes a new QuarchSource.
    pub fn new(ip: IpAddr, quarch_port: u16, sample: u32, metric: TypedMetricId<f64>) -> Self {
        QuarchSource {
            quarch_ip: ip,
            quarch_port,
            sample,
            metric,
            stream: None,
        }
    }

    /// Retrieves an environment variable or falls back to a default value for jdk & qis
    fn get_env_var_with_fallback(name: &str, fallback: &str) -> String {
        env::var(name).unwrap_or_else(|_| {
            log::debug!("Variable {} non defined, using fallback: {}", name, fallback);
            fallback.to_string()
        })
    }

    /// Ensures the QIS (Quarch Interface Service) is running, as it is required for communication with the Quarch device.
    ///
    /// This function is called during the plugin's start method. It terminates any existing QIS processes and starts a
    /// new instance using the specified Java binary and JAR path, guaranteeing a clean state before measurements begin.
    pub fn ensure_qis_running(qis_port: u16, java_bin: &str, qis_jar_path: &str) -> Result<Child> {
        let pids = get_qis_pids()?;
        if !pids.is_empty() {
            for pid in pids {
                let _ = Command::new("kill").arg("-9").arg(pid.to_string()).status();
            }
            sleep(Duration::from_secs(1));
        }

        let child = start_qis(java_bin, qis_jar_path)?;
        wait_for_qis_port("127.0.0.1", qis_port, 60)?;
        Ok(child)
    }

    /// Sends a command to the Quarch device and reads the response.
    fn send_quarch_command(&mut self, cmd: &str) -> Result<String> {
        // Send command
        let stream = self.stream.as_mut().context("not connected")?;
        let full_cmd = format!("{}\r\n", cmd);
        let mut message = Vec::new();
        message.push(full_cmd.len() as u8); // length
        message.push(0u8); // second byte = 0
        message.extend_from_slice(full_cmd.as_bytes());

        stream.write_all(&message)?;
        stream.flush()?;

        // Read response
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        let mut buffer = Vec::new();
        let mut tmp = [0u8; 1024];

        loop {
            match stream.read(&mut tmp) {
                Ok(n) if n > 0 => {
                    // Ignore the first 2 bytes of each packets (length)
                    if n > 2 {
                        buffer.extend_from_slice(&tmp[2..n]);
                    }
                    if buffer.ends_with(b"\r\n>") {
                        break;
                    }
                }
                Ok(_) => break,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {
                    log::error!("Reading timeout after 5s");
                    break;
                }
                Err(e) => return Err(anyhow!("Error on reading: {}", e)),
            }
        }

        if !buffer.is_empty() {
            let response = String::from_utf8_lossy(&buffer)
                .trim_end_matches("\r\n>")
                .trim()
                .to_string();
            Ok(response)
        } else {
            Err(anyhow!("Invalid or empty answer"))
        }
    }

    /// Parses a numeric value (e.g., voltage or current) from a string.
    fn extract_value(resp: &str) -> Option<f64> {
        resp.chars()
            .filter(|c| c.is_numeric() || *c == '.')
            .collect::<String>()
            .parse::<f64>()
            .ok()
    }

    /// Establishes a TCP connection to the Quarch device and configures it for measurements (e.g., setting averaging, trigger mode).
    fn connect_and_configure(&mut self) -> Result<()> {
        wait_for_qis_port(&self.quarch_ip.to_string(), self.quarch_port, 10)?;
        let stream = TcpStream::connect((self.quarch_ip, self.quarch_port))?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;

        self.stream = Some(stream);
        self.send_quarch_command("CONFig:DEFault STATE")?;
        self.send_quarch_command("RECord:TRIGger:MODE MANUAL")?;
        self.send_quarch_command(&format!("RECord:AVEraging {}K", self.sample))?;
        self.send_quarch_command("RECord:RUN")?;
        Ok(())
    }

    /// Retrieves power measurements from the Quarch device and returns them as a MeasureQuarch.
    fn get_measurement(&mut self) -> Result<MeasureQuarch> {
        let outputs = self.send_quarch_command("Measure:OUTputs?")?;
        let system = SystemTime::now();
        let timestamp = Timestamp::from(system);
        let mut total_voltage_mv: f64 = 0.0;
        let mut total_current_ua: f64 = 0.0;
        for line in outputs.lines() {
            if line.contains("mV")
                && let Some(voltage_str) = line.split('=').next_back()
                && let Some(voltage_value) = Self::extract_value(voltage_str.trim())
            {
                total_voltage_mv += voltage_value;
            } else if line.contains("uA")
                && let Some(current_str) = line.split('=').next_back()
                && let Some(current_value) = Self::extract_value(current_str.trim())
            {
                total_current_ua += current_value;
            }
        }
        let voltage = total_voltage_mv / 1000.0; // mV → V
        let current = total_current_ua / 1_000_000.0; // uA → A
        let power = voltage * current;

        Ok(MeasureQuarch { power, timestamp })
    }

    /// Stops ongoing power measurements on the Quarch device and closes the TCP connection.
    pub fn stop_measurement(&mut self) -> anyhow::Result<()> {
        //  Stopping measure
        if let Some(stream) = self.stream.take() {
            stream.set_write_timeout(Some(Duration::from_secs(2)))?;
            stream.set_read_timeout(Some(Duration::from_secs(2)))?;

            if let Err(e) = self.send_quarch_command("RECord:STOP") {
                log::error!("Failed to send RECord:STOP: {}", e);
            }
            if let Err(e) = self.send_quarch_command("$shutdown") {
                log::error!("Failed to send $shutdown: {}", e);
            }
            // Stopping TCP stream
            if let Err(e) = stream.shutdown(std::net::Shutdown::Both) {
                log::error!("Unable to shutdown TCP stream: {}", e);
            }
        }
        Ok(())
    }
}

/// Polls the Quarch device for measurements and accumulates them, handling connection errors.
impl Source for QuarchSource {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator<'_>, _timestamp: Timestamp) -> Result<(), PollError> {
        if self.stream.is_none()
            && let Err(e) = self.connect_and_configure()
        {
            log::error!("Impossible to connect with Quarch module: {}. Retrying next poll...", e);
            self.stream = None;
            return Err(PollError::CanRetry(e));
        }

        log::debug!("Polling QuarchSource...");
        match self.get_measurement() {
            Ok(data) => {
                let point = MeasurementPoint::new(
                    data.timestamp,
                    self.metric,
                    Resource::LocalMachine,
                    ResourceConsumer::LocalMachine,
                    data.power,
                );
                measurements.push(point);
            }
            Err(e) => {
                log::error!("Error with measure: {}. Disconnected and retry next poll.", e);
                self.stream = None;
                return Err(PollError::CanRetry(e));
            }
        }

        Ok(())
    }
}

/// Delegates polling to the wrapped QuarchSource in a thread-safe manner.
impl Source for SourceWrapper {
    fn poll(
        &mut self,
        measurements: &mut alumet::measurement::MeasurementAccumulator<'_>,
        timestamp: alumet::measurement::Timestamp,
    ) -> Result<(), alumet::pipeline::elements::source::PollError> {
        let mut s = self.inner.lock().unwrap();
        s.poll(measurements, timestamp)
    }
}

// --- Helper Functions ---

/// Retrieves the process IDs of running QIS instances to be able to stop it.
fn get_qis_pids() -> Result<Vec<i32>> {
    let output = Command::new("pgrep").arg("-f").arg("qis.jar").output()?;
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse::<i32>().ok())
        .collect())
}

/// Starts the QIS service using the specified JDK and JAR path.
fn start_qis(java_bin: &str, jar_path: &str) -> Result<Child> {
    let java_bin = QuarchSource::get_env_var_with_fallback("JAVA_HOME", java_bin);
    let jar_path = QuarchSource::get_env_var_with_fallback("QIS_JAR_PATH", jar_path);
    log::debug!("Starting QIS with JAVA_BIN={} and QIS_JAR_PATH={}", java_bin, jar_path);
    let child = Command::new(format!("{}/bin/java", java_bin))
        .arg("-jar")
        .arg(jar_path)
        .spawn()
        .context("Error on starting QIS")?;
    sleep(Duration::from_secs(3));
    Ok(child)
}

/// Waits for the QIS service to become available on a specified port, with a timeout.
fn wait_for_qis_port(ip: &str, port: u16, timeout_secs: u64) -> Result<()> {
    let start = Instant::now();
    let timeout = Duration::from_secs(timeout_secs);
    loop {
        if TcpStream::connect((ip, port)).is_ok() {
            log::debug!("QIS ready on port {}", port);
            return Ok(());
        }
        if start.elapsed() > timeout {
            return Err(anyhow!("Timeout: QIS doesn't listen on port {}", port));
        }
        sleep(Duration::from_millis(500));
    }
}
