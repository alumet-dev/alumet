use serde::Serialize;
use std::{fs::File, io, path::Path};

use crate::measure::parse::IoPressureFile;

/// Collects measurements from `io.pressure`.
///
/// The io.pressure file format is different from cpu.stat:
/// ```text
/// some avg10=0.00 avg60=0.00 avg300=0.00 total=91487491
/// full avg10=0.00 avg60=0.00 avg300=0.00 total=84675542
/// ```
///
/// Uses the dedicated IoPressureFile parser which handles the equals-separated
/// key-value format and the line-based structure (some/full prefixes).
pub struct IoPressureCollector {
    file: IoPressureFile,
}

#[derive(Debug, Serialize, Clone, Copy)]
pub struct IoPressureCollectorSettings {
    pub some_total: bool,
    pub full_total: bool,
}

impl Default for IoPressureCollectorSettings {
    fn default() -> Self {
        Self {
            some_total: true,
            full_total: true,
        }
    }
}

/// Represents the measurements extracted from the `io.pressure` file.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct IoPressureStats {
    pub some_total: Option<u64>,
    pub full_total: Option<u64>,
}

pub type CollectorCreationError = super::memory::CollectorCreationError;

impl IoPressureCollector {
    pub fn new<P: AsRef<Path>>(
        path: P,
        settings: IoPressureCollectorSettings,
        _io_buf: &mut Vec<u8>,
    ) -> Result<Self, CollectorCreationError> {
        let path = path.as_ref();
        let file = File::open(path).map_err(|e| CollectorCreationError::Io(e, path.into()))?;

        Ok(Self {
            file: IoPressureFile::new(file, settings),
        })
    }

    /// Collects measurements from the io.pressure file and computes the delta values.
    ///
    /// Uses the IoPressureFile parser to read the current values, then computes
    /// the delta since the last measurement.
    pub fn measure(&mut self, io_buf: &mut Vec<u8>) -> io::Result<IoPressureStats> {
        let current_stats = unsafe { self.file.read(io_buf) }?;
        Ok(current_stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    /// Creates a temporary io.pressure file with the given content.
    fn create_temp_io_pressure_file(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(content.as_bytes()).unwrap();
        file.flush().unwrap();
        file
    }

    mod io_pressure_settings {
        use super::*;

        #[test]
        fn test_io_pressure_collector_settings_default() {
            let settings = IoPressureCollectorSettings::default();
            assert!(settings.some_total);
            assert!(settings.full_total);
        }

        #[test]
        fn test_io_pressure_collector_settings_custom() {
            let settings = IoPressureCollectorSettings {
                some_total: false,
                full_total: true,
            };
            assert!(!settings.some_total);
            assert!(settings.full_total);
        }
    }

    mod io_pressure_stats {
        use super::*;

        #[test]
        fn test_io_pressure_stats_default() {
            let stats = IoPressureStats::default();
            assert_eq!(stats.some_total, None);
            assert_eq!(stats.full_total, None);
        }

        #[test]
        fn test_io_pressure_stats_equality() {
            let stats1 = IoPressureStats {
                some_total: Some(100),
                full_total: Some(200),
            };
            let stats2 = IoPressureStats {
                some_total: Some(100),
                full_total: Some(200),
            };
            assert_eq!(stats1, stats2);
        }

        #[test]
        fn test_io_pressure_stats_inequality() {
            let stats1 = IoPressureStats {
                some_total: Some(100),
                full_total: Some(200),
            };
            let stats2 = IoPressureStats {
                some_total: Some(100),
                full_total: Some(300),
            };
            assert_ne!(stats1, stats2);
        }
    }

    mod io_pressure_collector {
        use super::*;

        #[test]
        fn test_io_pressure_collector_new_valid_file() {
            let content = "some avg10=0.00 avg60=0.00 avg300=0.00 total=91487491\n\
                           full avg10=0.00 avg60=0.00 avg300=0.00 total=84675542\n";
            let temp_file = create_temp_io_pressure_file(content);
            let mut io_buf = Vec::new();
            let settings = IoPressureCollectorSettings::default();

            let result = IoPressureCollector::new(temp_file.path(), settings, &mut io_buf);
            assert!(result.is_ok());
        }

        #[test]
        fn test_io_pressure_collector_new_nonexistent_file() {
            let mut io_buf = Vec::new();
            let settings = IoPressureCollectorSettings::default();

            let result = IoPressureCollector::new("/nonexistent/path/io.pressure", settings, &mut io_buf);
            assert!(result.is_err());
        }

        #[test]
        fn test_io_pressure_collector_measure_full_content() {
            let content = "some avg10=0.00 avg60=0.00 avg300=0.00 total=91487491\n\
                           full avg10=0.00 avg60=0.00 avg300=0.00 total=84675542\n";
            let temp_file = create_temp_io_pressure_file(content);
            let mut io_buf = Vec::new();
            let settings = IoPressureCollectorSettings::default();
            let mut collector = IoPressureCollector::new(temp_file.path(), settings, &mut io_buf).unwrap();

            let result = collector.measure(&mut io_buf);
            assert!(result.is_ok());

            let stats = result.unwrap();
            assert_eq!(stats.some_total, Some(91487491));
            assert_eq!(stats.full_total, Some(84675542));
        }

        #[test]
        fn test_io_pressure_collector_measure_only_some() {
            let content = "some avg10=0.00 avg60=0.00 avg300=0.00 total=12345678\n\
            full avg10=0.00 avg60=0.00 avg300=0.00 total=84675542\n";
            let temp_file = create_temp_io_pressure_file(content);
            let mut io_buf = Vec::new();
            let settings = IoPressureCollectorSettings {
                some_total: true,
                full_total: false,
            };
            let mut collector = IoPressureCollector::new(temp_file.path(), settings, &mut io_buf).unwrap();

            let result = collector.measure(&mut io_buf);
            assert!(result.is_ok());

            let stats = result.unwrap();
            assert_eq!(stats.some_total, Some(12345678));
            assert_eq!(stats.full_total, None);
        }

        #[test]
        fn test_io_pressure_collector_measure_only_full() {
            let content = "full avg10=0.00 avg60=0.00 avg300=0.00 total=98765432\n";
            let temp_file = create_temp_io_pressure_file(content);
            let mut io_buf = Vec::new();
            let settings = IoPressureCollectorSettings::default();
            let mut collector = IoPressureCollector::new(temp_file.path(), settings, &mut io_buf).unwrap();

            let result = collector.measure(&mut io_buf);
            assert!(result.is_ok());

            let stats = result.unwrap();
            assert_eq!(stats.some_total, None);
            assert_eq!(stats.full_total, Some(98765432));
        }

        #[test]
        fn test_io_pressure_collector_measure_empty_file() {
            let content = "";
            let temp_file = create_temp_io_pressure_file(content);
            let mut io_buf = Vec::new();
            let settings = IoPressureCollectorSettings::default();
            let mut collector = IoPressureCollector::new(temp_file.path(), settings, &mut io_buf).unwrap();

            let result = collector.measure(&mut io_buf);
            assert!(result.is_ok());

            let stats = result.unwrap();
            assert_eq!(stats.some_total, None);
            assert_eq!(stats.full_total, None);
        }

        #[test]
        fn test_io_pressure_collector_measure_with_zero_values() {
            let content = "some avg10=0.00 avg60=0.00 avg300=0.00 total=0\n\
                           full avg10=0.00 avg60=0.00 avg300=0.00 total=0\n";
            let temp_file = create_temp_io_pressure_file(content);
            let mut io_buf = Vec::new();
            let settings = IoPressureCollectorSettings::default();
            let mut collector = IoPressureCollector::new(temp_file.path(), settings, &mut io_buf).unwrap();

            let result = collector.measure(&mut io_buf);
            assert!(result.is_ok());

            let stats = result.unwrap();
            assert_eq!(stats.some_total, Some(0));
            assert_eq!(stats.full_total, Some(0));
        }

        #[test]
        fn test_io_pressure_collector_measure_with_large_values() {
            let content = "some avg10=0.00 avg60=0.00 avg300=0.00 total=18446744073709551615\n\
                           full avg10=0.00 avg60=0.00 avg300=0.00 total=18446744073709551614\n";
            let temp_file = create_temp_io_pressure_file(content);
            let mut io_buf = Vec::new();
            let settings = IoPressureCollectorSettings::default();
            let mut collector = IoPressureCollector::new(temp_file.path(), settings, &mut io_buf).unwrap();

            let result = collector.measure(&mut io_buf);
            assert!(result.is_ok());

            let stats = result.unwrap();
            assert_eq!(stats.some_total, Some(u64::MAX));
            assert_eq!(stats.full_total, Some(u64::MAX - 1));
        }

        #[test]
        fn test_io_pressure_collector_measure_with_extra_whitespace() {
            let content = "some avg10=0.00 avg60=0.00 avg300=0.00 total=100  \n\
                           full avg10=0.00 avg60=0.00 avg300=0.00 total=200  \n";
            let temp_file = create_temp_io_pressure_file(content);
            let mut io_buf = Vec::new();
            let settings = IoPressureCollectorSettings::default();
            let mut collector = IoPressureCollector::new(temp_file.path(), settings, &mut io_buf).unwrap();

            let result = collector.measure(&mut io_buf);
            assert!(result.is_ok());

            let stats = result.unwrap();
            assert_eq!(stats.some_total, Some(100));
            assert_eq!(stats.full_total, Some(200));
        }

        #[test]
        fn test_io_pressure_collector_measure_with_mixed_line_endings() {
            let content = "some avg10=0.00 avg60=0.00 avg300=0.00 total=150\r\n\
                           full avg10=0.00 avg60=0.00 avg300=0.00 total=250\r\n";
            let temp_file = create_temp_io_pressure_file(content);
            let mut io_buf = Vec::new();
            let settings = IoPressureCollectorSettings::default();
            let mut collector = IoPressureCollector::new(temp_file.path(), settings, &mut io_buf).unwrap();

            let result = collector.measure(&mut io_buf);
            assert!(result.is_ok());

            let stats = result.unwrap();
            assert_eq!(stats.some_total, Some(150));
            assert_eq!(stats.full_total, Some(250));
        }

        #[test]
        fn test_io_pressure_collector_measure_multiple_calls() {
            let content = "some avg10=0.00 avg60=0.00 avg300=0.00 total=100\n\
                           full avg10=0.00 avg60=0.00 avg300=0.00 total=200\n";
            let temp_file = create_temp_io_pressure_file(content);
            let mut io_buf = Vec::new();
            let settings = IoPressureCollectorSettings::default();
            let mut collector = IoPressureCollector::new(temp_file.path(), settings, &mut io_buf).unwrap();

            // First measurement
            let result1 = collector.measure(&mut io_buf);
            assert!(result1.is_ok());
            let stats1 = result1.unwrap();
            assert_eq!(stats1.some_total, Some(100));
            assert_eq!(stats1.full_total, Some(200));

            // Second measurement (should read the same values again)
            let result2 = collector.measure(&mut io_buf);
            assert!(result2.is_ok());
            let stats2 = result2.unwrap();
            assert_eq!(stats2.some_total, Some(100));
            assert_eq!(stats2.full_total, Some(200));
        }

        #[test]
        fn test_io_pressure_collector_measure_buffer_reuse() {
            let content = "some avg10=0.00 avg60=0.00 avg300=0.00 total=300\n\
                           full avg10=0.00 avg60=0.00 avg300=0.00 total=400\n";
            let temp_file = create_temp_io_pressure_file(content);
            let mut io_buf = Vec::new();
            let settings = IoPressureCollectorSettings::default();
            let mut collector = IoPressureCollector::new(temp_file.path(), settings, &mut io_buf).unwrap();

            // First call with empty buffer
            let result1 = collector.measure(&mut io_buf);
            assert!(result1.is_ok());

            // Second call with non-empty buffer (should still work)
            let result2 = collector.measure(&mut io_buf);
            assert!(result2.is_ok());

            let stats = result2.unwrap();
            assert_eq!(stats.some_total, Some(300));
            assert_eq!(stats.full_total, Some(400));
        }
    }

    mod io_pressure_collector_settings {
        use super::*;

        #[test]
        fn test_io_pressure_collector_with_path_str() {
            let content = "some avg10=0.00 avg60=0.00 avg300=0.00 total=500\n\
                           full avg10=0.00 avg60=0.00 avg300=0.00 total=600\n";
            let temp_file = create_temp_io_pressure_file(content);
            let mut io_buf = Vec::new();
            let settings = IoPressureCollectorSettings::default();

            // Test with &str path
            let result = IoPressureCollector::new(temp_file.path().to_str().unwrap(), settings, &mut io_buf);
            assert!(result.is_ok());
        }

        #[test]
        fn test_io_pressure_collector_with_path_buf() {
            let content = "some avg10=0.00 avg60=0.00 avg300=0.00 total=700\n\
                           full avg10=0.00 avg60=0.00 avg300=0.00 total=800\n";
            let temp_file = create_temp_io_pressure_file(content);
            let mut io_buf = Vec::new();
            let settings = IoPressureCollectorSettings::default();

            // Test with PathBuf
            let result = IoPressureCollector::new(temp_file.path().to_path_buf(), settings, &mut io_buf);
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_io_pressure_full() {
        let content = "some avg10=0.00 avg60=0.00 avg300=0.00 total=91487491\n\
                        full avg10=0.00 avg60=0.00 avg300=0.00 total=84675542\n";
        let temp_file = create_temp_io_pressure_file(content);
        let mut io_buf = Vec::new();
        let settings = IoPressureCollectorSettings::default();

        let result = IoPressureCollector::new(temp_file.path(), settings, &mut io_buf.clone());
        assert!(result.is_ok());
        let read_pressure_values = unsafe { result.unwrap().file.read(&mut io_buf) }.unwrap();
        assert!(read_pressure_values.full_total.is_some());
        assert!(read_pressure_values.some_total.is_some());
        assert_eq!(read_pressure_values.some_total, Some(91487491));
        assert_eq!(read_pressure_values.full_total, Some(84675542));
    }
}
