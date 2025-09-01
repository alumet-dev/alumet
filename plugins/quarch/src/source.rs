// This file implements the source functionality for the Quarch input plugin.
use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint, Timestamp},
    metrics::TypedMetricId,
    pipeline::elements::source::{PollError, Source},
    resources::{Resource, ResourceConsumer},
};
use anyhow::{Context, Result, anyhow};
use log::{debug, error};
use std::{
    env,
    io::{Read, Write},
    net::{IpAddr, TcpStream},
    process::{Child, Command},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::sleep,
    time::{Duration, Instant, SystemTime},
};

#[derive(Debug)]
pub struct MeasureQuarch {
    pub power: f64,
    pub timestamp: Timestamp,
}

pub struct QuarchSource {
    quarch_ip: IpAddr,
    quarch_port: u16,
    sample: u32,
    metric: TypedMetricId<f64>,
    pub(crate) stream: Option<TcpStream>,
    stop_flag: Arc<AtomicBool>, // to stop polling when end event is triggered
    already_stopped: Arc<AtomicBool>,
}

pub struct SourceWrapper {
    pub(crate) inner: Arc<Mutex<QuarchSource>>,
}

impl QuarchSource {
    pub fn new(ip: IpAddr, quarch_port: u16, sample: u32, metric: TypedMetricId<f64>) -> Self {
        QuarchSource {
            quarch_ip: ip,
            quarch_port,
            sample,
            metric,
            stream: None,
            stop_flag: Arc::new(AtomicBool::new(false)),
            already_stopped: Arc::new(AtomicBool::new(false)),
        }
    }

    /// To get the environment variable for jdk & qis
    fn get_env_var_with_fallback(name: &str, fallback: &str) -> String {
        env::var(name).unwrap_or_else(|_| {
            debug!("Variable {} non defined, using fallback: {}", name, fallback);
            fallback.to_string()
        })
    }

    /// Start QIS if necessary on our side
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

    /// Used to both send command and get the response of it (if added)
    fn send_quarch_command(&mut self, cmd: &str) -> Result<String> {
        // Send command
        let stream = self.stream.as_mut().ok_or_else(|| anyhow!("Not connected"))?;
        let full_cmd = format!("{}\r\n", cmd);
        let mut message = Vec::new();
        message.push(full_cmd.len() as u8); // longueur
        message.push(0u8); // second octet = 0
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
                    // Ignore the first 2 bytes of each packets
                    if n > 2 {
                        buffer.extend_from_slice(&tmp[2..n]);
                    }
                    if buffer.ends_with(b"\r\n>") {
                        break;
                    }
                }
                Ok(_) => break,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {
                    error!("Timeout de lecture après 5s");
                    break;
                }
                Err(e) => return Err(anyhow!("Erreur lecture: {}", e)),
            }
        }

        if !buffer.is_empty() {
            let response = String::from_utf8_lossy(&buffer)
                .trim_end_matches("\r\n>")
                .trim()
                .to_string();
            Ok(response)
        } else {
            Err(anyhow!("Réponse invalide ou vide"))
        }
    }

    fn extract_value(resp: &str) -> Option<f64> {
        resp.chars()
            .filter(|c| c.is_numeric() || *c == '.')
            .collect::<String>()
            .parse::<f64>()
            .ok()
    }

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

    fn get_measurement(&mut self) -> Result<MeasureQuarch> {
        let system = SystemTime::now();
        let timestamp = Timestamp::from(system);
        let outputs = self.send_quarch_command("Measure:OUTputs?")?;
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
    pub fn stop_measurement(&mut self) -> anyhow::Result<()> {
        if self.already_stopped.swap(true, Ordering::SeqCst) {
            log::debug!("stop_measurement() ignored: already stoppé.");
            return Ok(());
        }

        self.stop_flag.store(true, Ordering::SeqCst);

        //  Stopping measure
        if let Some(stream) = self.stream.as_ref() {
            stream.set_write_timeout(Some(Duration::from_secs(2)))?;
            if let Err(e) = self.send_quarch_command("RECord:STOP") {
                log::error!("Failed to send RECord:STOP: {}", e);
            }
            if let Err(e) = self.send_quarch_command("$shutdown") {
                log::error!("Failed to send $shutdown: {}", e);
            }
        }

        // Stopping TCP stream
        if let Some(stream) = self.stream.take() {
            stream.set_read_timeout(Some(Duration::from_secs(2)))?;
            if let Err(e) = stream.shutdown(std::net::Shutdown::Both) {
                log::error!("Unable to shutdown TCP stream: {}", e);
            }
        }
        Ok(())
    }
}

impl Source for QuarchSource {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator<'_>, _timestamp: Timestamp) -> Result<(), PollError> {
        if self.stop_flag.load(Ordering::SeqCst) {
            log::debug!("Polling skipped: stop flag is active.");
            return Ok(()); // Stop polling
        }

        if self.stream.is_none()
            && let Err(e) = self.connect_and_configure()
        {
            error!("Impossible to connect with Quarch module: {}. Retrying next poll...", e);
            self.stream = None;
            return Ok(());
        }

        debug!("Polling QuarchSource...");
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
                error!("Error with measure: {}. Disconnected and retry next poll.", e);
                self.stream = None;
                return Ok(());
            }
        }

        Ok(())
    }
}

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

fn get_qis_pids() -> Result<Vec<i32>> {
    let output = Command::new("pgrep").arg("-f").arg("qis.jar").output()?;
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse::<i32>().ok())
        .collect())
}

fn start_qis(java_bin: &str, jar_path: &str) -> Result<Child> {
    let java_bin = QuarchSource::get_env_var_with_fallback("JAVA_HOME", java_bin);
    let jar_path = QuarchSource::get_env_var_with_fallback("QIS_JAR_PATH", jar_path);
    debug!("Starting QIS with JAVA_BIN={} and QIS_JAR_PATH={}", java_bin, jar_path);
    let child = Command::new(format!("{}/bin/java", java_bin))
        .arg("-jar")
        .arg(jar_path)
        .spawn()
        .context("Error on starting QIS")?;
    sleep(Duration::from_secs(3));
    Ok(child)
}

fn wait_for_qis_port(ip: &str, port: u16, timeout_secs: u64) -> Result<()> {
    let start = Instant::now();
    loop {
        if TcpStream::connect((ip, port)).is_ok() {
            debug!("QIS ready on port {}", port);
            return Ok(());
        }
        if start.elapsed() > Duration::from_secs(timeout_secs) {
            return Err(anyhow!("Timeout: QIS doesn't listen on port {}", port));
        }
        sleep(Duration::from_millis(500));
    }
}
