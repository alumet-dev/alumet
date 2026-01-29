//! System-level network metrics read from `/proc/net/dev`.

use std::{
    fs::File,
    io::{BufReader, ErrorKind, Seek},
    path::PathBuf,
};

use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint, Timestamp},
    metrics::{TypedMetricId, error::MetricCreationError},
    pipeline::{Source, elements::error::PollError},
    plugin::AlumetPluginStart,
    resources::{Resource, ResourceConsumer},
    units::Unit,
};
use anyhow::Context;
use procfs::{
    FromBufRead,
    net::{DeviceStatus, InterfaceDeviceStatus},
};

/// Reads network metrics from /proc/net/dev
pub struct NetworkProbe {
    path: PathBuf,
    reader: ProcNetReader,
    previous: Option<InterfaceDeviceStatus>,
    metrics: NetworkMetrics,
}

/// How to read `/proc/net/dev`.
enum ProcNetReader {
    /// Optimized version, where we open the file only once.
    Optimized(BufReader<File>),
    /// Less optimized version where the file is opened each time, to work around a bug.
    Workaround,
}

pub struct NetworkMetrics {
    // More metrics available as part of InterfaceDeviceStatus but not considered for now
    pub bytes: TypedMetricId<u64>,
    pub packets: TypedMetricId<u64>,
    pub drops: TypedMetricId<u64>,
    pub errors: TypedMetricId<u64>,
}

impl NetworkMetrics {
    pub fn new(alumet: &mut AlumetPluginStart) -> Result<Self, MetricCreationError> {
        Ok(Self {
            bytes: alumet.create_metric("network_bytes", Unit::Byte, "Number of bytes (rx/tx) per interface")?,
            packets: alumet.create_metric(
                "network_packets",
                Unit::Unity,
                "Number of packets (rx/tx) per interface",
            )?,
            drops: alumet.create_metric(
                "network_packet_drops",
                Unit::Unity,
                "Number of dropped packets (rx/tx) per interface",
            )?,
            errors: alumet.create_metric("network_errors", Unit::Unity, "Number of errors (rx/tx) per interface")?,
        })
    }
}

impl ProcNetReader {
    fn read(&mut self, path: &PathBuf) -> anyhow::Result<InterfaceDeviceStatus> {
        let res = match self {
            ProcNetReader::Optimized(reader) => {
                reader.rewind().with_context(|| format!("failed to rewind {path:?}"))?;
                InterfaceDeviceStatus::from_buf_read(reader)
            }
            ProcNetReader::Workaround => {
                // A kernel bug makes rewind fail on /proc/net/* (see issue #336) in some versions.
                // Workaround: reopen the file each time.
                let file = File::open(&path).with_context(|| format!("cannot open {path:?}"))?;
                let mut reader = BufReader::new(file);
                InterfaceDeviceStatus::from_buf_read(&mut reader)
            }
        };
        res.with_context(|| format!("error while parsing {path:?}"))
    }
}

impl NetworkProbe {
    pub fn new(metrics: NetworkMetrics, path: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let path = path.into();
        let mut file = File::open(&path).with_context(|| format!("cannot open {path:?}"))?;
        let reader = match file.rewind() {
            Ok(_) => ProcNetReader::Optimized(BufReader::new(file)),
            Err(e) if e.kind() == ErrorKind::NotSeekable => {
                log::warn!(
                    "Cannot rewind on {path:?}, your kernel probably has a bug (see issue #336). A workaround will be used, but the network source will be slower."
                );
                ProcNetReader::Workaround
            }
            Err(bad) => return Err(anyhow::Error::new(bad).context(format!("cannot rewind {path:?}"))),
        };
        Ok(Self {
            path,
            reader,
            previous: None,
            metrics,
        })
    }
}

impl Source for NetworkProbe {
    fn poll(&mut self, acc: &mut MeasurementAccumulator, ts: Timestamp) -> Result<(), PollError> {
        let now = self.reader.read(&self.path)?;
        // Only push deltas, not the baseline value before the plugin starts
        if let Some(ref prev) = self.previous {
            for (if_name, now_stats) in &now.0 {
                // Also consider the first value when a new interface appears at runtime
                let prev_stats = match prev.0.get(if_name) {
                    Some(p) => p,
                    // Default not implemented by procfs
                    None => &DeviceStatus {
                        name: if_name.clone(),
                        recv_bytes: 0,
                        recv_packets: 0,
                        recv_errs: 0,
                        recv_drop: 0,
                        sent_bytes: 0,
                        sent_packets: 0,
                        sent_errs: 0,
                        sent_drop: 0,
                        recv_fifo: 0,
                        recv_frame: 0,
                        recv_compressed: 0,
                        recv_multicast: 0,
                        sent_fifo: 0,
                        sent_colls: 0,
                        sent_carrier: 0,
                        sent_compressed: 0,
                    },
                };
                //};
                let res = Resource::LocalMachine;
                let cons = ResourceConsumer::LocalMachine;

                // ---------- rx ----------
                acc.push(
                    MeasurementPoint::new(
                        ts,
                        self.metrics.bytes,
                        res.clone(),
                        cons.clone(),
                        now_stats.recv_bytes - prev_stats.recv_bytes,
                    )
                    .with_attr("interface", if_name.clone())
                    .with_attr("direction", "rx"),
                );
                acc.push(
                    MeasurementPoint::new(
                        ts,
                        self.metrics.packets,
                        res.clone(),
                        cons.clone(),
                        now_stats.recv_packets - prev_stats.recv_packets,
                    )
                    .with_attr("interface", if_name.clone())
                    .with_attr("direction", "rx"),
                );
                acc.push(
                    MeasurementPoint::new(
                        ts,
                        self.metrics.drops,
                        res.clone(),
                        cons.clone(),
                        now_stats.recv_drop - prev_stats.recv_drop,
                    )
                    .with_attr("interface", if_name.clone())
                    .with_attr("direction", "rx"),
                );
                acc.push(
                    MeasurementPoint::new(
                        ts,
                        self.metrics.errors,
                        res.clone(),
                        cons.clone(),
                        now_stats.recv_errs - prev_stats.recv_errs,
                    )
                    .with_attr("interface", if_name.clone())
                    .with_attr("direction", "rx"),
                );

                // ---------- tx ----------
                acc.push(
                    MeasurementPoint::new(
                        ts,
                        self.metrics.bytes,
                        res.clone(),
                        cons.clone(),
                        now_stats.sent_bytes - prev_stats.sent_bytes,
                    )
                    .with_attr("interface", if_name.clone())
                    .with_attr("direction", "tx"),
                );
                acc.push(
                    MeasurementPoint::new(
                        ts,
                        self.metrics.packets,
                        res.clone(),
                        cons.clone(),
                        now_stats.sent_packets - prev_stats.sent_packets,
                    )
                    .with_attr("interface", if_name.clone())
                    .with_attr("direction", "tx"),
                );
                acc.push(
                    MeasurementPoint::new(
                        ts,
                        self.metrics.drops,
                        res.clone(),
                        cons.clone(),
                        now_stats.sent_drop - prev_stats.sent_drop,
                    )
                    .with_attr("interface", if_name.clone())
                    .with_attr("direction", "tx"),
                );
                acc.push(
                    MeasurementPoint::new(
                        ts,
                        self.metrics.errors,
                        res.clone(),
                        cons.clone(),
                        now_stats.sent_errs - prev_stats.sent_errs,
                    )
                    .with_attr("interface", if_name.clone())
                    .with_attr("direction", "tx"),
                );
            }
        }

        self.previous = Some(now);
        Ok(())
    }
}
