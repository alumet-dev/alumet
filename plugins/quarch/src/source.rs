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
    io::{ErrorKind, Read, Write},
    net::{IpAddr, TcpStream},
    process::{Child, Command},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::sleep,
    time::{Duration, Instant},
};

// Fallback path by default (maybe put it in config?)
const DEFAULT_JAVA_BIN: &str =
    "/root/venv-quarchpy/lib/python3.11/site-packages/quarchpy/connection_specific/jdk_jres/lin_amd64_jdk_jre/bin/java";
const DEFAULT_QIS_JAR_PATH: &str =
    "/root/venv-quarchpy/lib/python3.11/site-packages/quarchpy/connection_specific/QPS/win-amd64/qis/qis.jar";
const QIS_PORT: u16 = 9780;

#[derive(Debug)]
pub struct MeasureQuarch {
    pub power: f64,
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
    pub fn new(ip: IpAddr, port: u16, sample: u32, metric: TypedMetricId<f64>) -> Self {
        QuarchSource {
            quarch_ip: ip,
            quarch_port: port,
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

    /// Start QIS if necessary
    pub fn ensure_qis_running() -> Result<Child> {
        let pids = get_qis_pids()?;
        if !pids.is_empty() {
            for pid in pids {
                let _ = Command::new("kill").arg("-9").arg(pid.to_string()).status();
            }
            sleep(Duration::from_secs(1));
        }

        let child = start_qis()?;
        //info!("Wait for QIS to listen on port {}...", QIS_PORT);
        wait_for_qis_port("127.0.0.1", QIS_PORT, 60)?;
        //info!("QIS ready.");
        Ok(child)
    }

    fn send_command(&mut self, cmd: &str) -> Result<()> {
        let stream = self.stream.as_mut().ok_or_else(|| anyhow!("Not connected"))?;
        let full_cmd = format!("{}\r\n", cmd);
        let mut message = Vec::new();
        message.push(full_cmd.len() as u8);
        message.push(0u8);
        message.extend_from_slice(full_cmd.as_bytes());
        stream.write_all(&message)?;
        stream.flush()?;
        Ok(())
    }

    fn read_response(&mut self) -> Result<String> {
        let stream = self.stream.as_mut().ok_or_else(|| anyhow!("Not connected"))?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        let mut buffer = Vec::new();
        let mut tmp = [0u8; 1024];
        loop {
            match stream.read(&mut tmp) {
                Ok(n) if n > 0 => {
                    if n > 2 {
                        buffer.extend_from_slice(&tmp[2..n]);
                    }
                    if buffer.ends_with(b"\r\n>") {
                        break;
                    }
                }
                Ok(_) => break,
                Err(ref e) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => {
                    error!("Reading timeout after 5s");
                    break;
                }
                Err(e) => return Err(anyhow!("Erreur on reading: {}", e)),
            }
        }
        if buffer.is_empty() {
            Err(anyhow!("Empty answer"))
        } else {
            Ok(String::from_utf8_lossy(&buffer)
                .trim_end_matches("\r\n>")
                .trim()
                .to_string())
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
        //info!("Connection the quarch module on {}:{}", self.quarch_ip, self.quarch_port);
        wait_for_qis_port(&self.quarch_ip.to_string(), self.quarch_port, 30)?;
        let stream = TcpStream::connect((self.quarch_ip, self.quarch_port))?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;

        self.stream = Some(stream);
        //info!("Connection established, init configuration");
        self.send_command("CONFig:DEFault STATE")?;
        self.send_command("RECord:TRIGger:MODE MANUAL")?;
        self.send_command(&format!("RECord:AVEraging {}K", self.sample))?;
        self.send_command("RECord:RUN")?;
        Ok(())
    }

    fn get_measurement(&mut self) -> Result<MeasureQuarch> {
        self.send_command("MEASure:VOLTage +12V?")?;
        let voltage_resp = self.read_response()?;
        let voltage = Self::extract_value(&voltage_resp).unwrap_or(0.0) / 1000.0;
        self.send_command("MEASure:CURRent +12V?")?;
        let current_resp = self.read_response()?;
        let current = Self::extract_value(&current_resp).unwrap_or(0.0) / 1_000_000.0;
        Ok(MeasureQuarch {
            power: voltage * current,
        })
    }
    pub fn stop_measurement(&mut self) -> anyhow::Result<()> {
        if self.already_stopped.swap(true, Ordering::SeqCst) {
            log::debug!("stop_measurement() ignored: already stoppé.");
            return Ok(());
        }

        //log::info!("Entering stop_measurement...");
        self.stop_flag.store(true, Ordering::SeqCst);

        //  Stopping measure
        if let Some(stream) = self.stream.as_ref() {
            stream.set_write_timeout(Some(Duration::from_secs(2)))?;
            if let Err(e) = self.send_command("RECord:STOP") {
                log::error!("Failed to send RECord:STOP: {}", e);
            }
            if let Err(e) = self.send_command("$shutdown") {
                log::error!("Failed to send $shutdown: {}", e);
            }
        }

        // Stopping TCP stream
        if let Some(stream) = self.stream.take() {
            //log::info!("Shutting down TCP stream...");
            stream.set_read_timeout(Some(Duration::from_secs(2)))?;
            if let Err(e) = stream.shutdown(std::net::Shutdown::Both) {
                log::error!("Unable to shutdown TCP stream: {}", e);
            }
        }
        Ok(())
    }
}

impl Source for QuarchSource {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator<'_>, timestamp: Timestamp) -> Result<(), PollError> {
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
                //info!("Measurement received: {} W", data.power);
                let point = MeasurementPoint::new(
                    timestamp,
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

fn start_qis() -> Result<Child> {
    let java_bin = QuarchSource::get_env_var_with_fallback("JAVA_HOME", DEFAULT_JAVA_BIN);
    let jar_path = QuarchSource::get_env_var_with_fallback("QIS_JAR_PATH", DEFAULT_QIS_JAR_PATH);
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
