use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct SensorId {
    pub source: String,
    pub chip: String,
    pub sensor: String,
}

impl std::fmt::Display for SensorId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}/{}", self.source, self.chip, self.sensor)
    }
}

impl SensorId {
    /// Natural sort comparison: treats numeric suffixes numerically
    /// so "cpu2" < "cpu10" instead of lexicographic "cpu10" < "cpu2".
    pub fn natural_cmp(&self, other: &Self) -> std::cmp::Ordering {
        natural_cmp_str(&self.source, &other.source)
            .then_with(|| natural_cmp_str(&self.chip, &other.chip))
            .then_with(|| natural_cmp_str(&self.sensor, &other.sensor))
    }
}

pub fn natural_cmp_str(a: &str, b: &str) -> std::cmp::Ordering {
    let mut ai = a.chars().peekable();
    let mut bi = b.chars().peekable();

    loop {
        match (ai.peek(), bi.peek()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(&ac), Some(&bc)) => {
                if ac.is_ascii_digit() && bc.is_ascii_digit() {
                    let an = consume_number(&mut ai);
                    let bn = consume_number(&mut bi);
                    match an.cmp(&bn) {
                        std::cmp::Ordering::Equal => continue,
                        ord => return ord,
                    }
                }
                match ac.cmp(&bc) {
                    std::cmp::Ordering::Equal => {
                        ai.next();
                        bi.next();
                    }
                    ord => return ord,
                }
            }
        }
    }
}

fn consume_number(iter: &mut std::iter::Peekable<std::str::Chars<'_>>) -> u64 {
    let mut n: u64 = 0;
    while let Some(&c) = iter.peek() {
        if c.is_ascii_digit() {
            n = n.saturating_mul(10).saturating_add(c as u64 - '0' as u64);
            iter.next();
        } else {
            break;
        }
    }
    n
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorReading {
    pub label: String,
    pub current: f64,
    pub unit: SensorUnit,
    pub category: SensorCategory,
    pub min: f64,
    pub max: f64,
    pub avg: f64,
    pub sample_count: u64,
    pub last_updated: DateTime<Utc>,
}

impl SensorReading {
    pub fn new(label: String, value: f64, unit: SensorUnit, category: SensorCategory) -> Self {
        Self {
            label,
            current: value,
            unit,
            category,
            min: value,
            max: value,
            avg: value,
            sample_count: 1,
            last_updated: Utc::now(),
        }
    }

    pub fn update(&mut self, value: f64) {
        self.current = value;
        self.min = self.min.min(value);
        self.max = self.max.max(value);
        self.sample_count += 1;
        self.avg += (value - self.avg) / self.sample_count as f64;
        self.last_updated = Utc::now();
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum SensorCategory {
    Temperature,
    Voltage,
    Current,
    Power,
    Fan,
    Frequency,
    Utilization,
    Throughput,
    Memory,
    Other,
}

impl SensorCategory {
    /// Stable display ordering for TUI tree layout.
    pub fn sort_key(self) -> u8 {
        match self {
            Self::Temperature => 0,
            Self::Voltage => 1,
            Self::Fan => 2,
            Self::Power => 3,
            Self::Current => 4,
            Self::Frequency => 5,
            Self::Utilization => 6,
            Self::Throughput => 7,
            Self::Memory => 8,
            Self::Other => 9,
        }
    }
}

impl std::fmt::Display for SensorCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Temperature => write!(f, "Temperature"),
            Self::Voltage => write!(f, "Voltage"),
            Self::Current => write!(f, "Current"),
            Self::Power => write!(f, "Power"),
            Self::Fan => write!(f, "Fan"),
            Self::Frequency => write!(f, "Frequency"),
            Self::Utilization => write!(f, "Utilization"),
            Self::Throughput => write!(f, "Throughput"),
            Self::Memory => write!(f, "Memory"),
            Self::Other => write!(f, "Other"),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum SensorUnit {
    Celsius,
    Millivolts,
    Volts,
    Milliamps,
    Amps,
    Watts,
    Milliwatts,
    Rpm,
    Mhz,
    Percent,
    BytesPerSec,
    MegabytesPerSec,
    Bytes,
    Megabytes,
    Unitless,
}

impl std::fmt::Display for SensorUnit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Celsius => write!(f, "\u{00b0}C"),
            Self::Millivolts => write!(f, "mV"),
            Self::Volts => write!(f, "V"),
            Self::Milliamps => write!(f, "mA"),
            Self::Amps => write!(f, "A"),
            Self::Watts => write!(f, "W"),
            Self::Milliwatts => write!(f, "mW"),
            Self::Rpm => write!(f, "RPM"),
            Self::Mhz => write!(f, "MHz"),
            Self::Percent => write!(f, "%"),
            Self::BytesPerSec => write!(f, "B/s"),
            Self::MegabytesPerSec => write!(f, "MB/s"),
            Self::Bytes => write!(f, "B"),
            Self::Megabytes => write!(f, "MB"),
            Self::Unitless => write!(f, ""),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorSnapshot {
    pub timestamp: DateTime<Utc>,
    pub readings: HashMap<SensorId, SensorReading>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sensor_reading_new() {
        let r = SensorReading::new(
            "Test".into(),
            42.0,
            SensorUnit::Celsius,
            SensorCategory::Temperature,
        );
        assert_eq!(r.current, 42.0);
        assert_eq!(r.min, 42.0);
        assert_eq!(r.max, 42.0);
        assert_eq!(r.avg, 42.0);
        assert_eq!(r.sample_count, 1);
    }

    #[test]
    fn test_sensor_reading_update() {
        let mut r = SensorReading::new(
            "Test".into(),
            10.0,
            SensorUnit::Celsius,
            SensorCategory::Temperature,
        );
        r.update(20.0);
        assert_eq!(r.current, 20.0);
        assert_eq!(r.min, 10.0);
        assert_eq!(r.max, 20.0);
        assert_eq!(r.sample_count, 2);
        assert!((r.avg - 15.0).abs() < 0.01);
    }

    #[test]
    fn test_natural_sort_basic() {
        let a = SensorId {
            source: "cpu".into(),
            chip: "freq".into(),
            sensor: "cpu2".into(),
        };
        let b = SensorId {
            source: "cpu".into(),
            chip: "freq".into(),
            sensor: "cpu10".into(),
        };
        assert_eq!(a.natural_cmp(&b), std::cmp::Ordering::Less);
    }

    #[test]
    fn test_natural_sort_same() {
        let a = SensorId {
            source: "a".into(),
            chip: "b".into(),
            sensor: "c1".into(),
        };
        let b = SensorId {
            source: "a".into(),
            chip: "b".into(),
            sensor: "c1".into(),
        };
        assert_eq!(a.natural_cmp(&b), std::cmp::Ordering::Equal);
    }

    #[test]
    fn test_natural_sort_sequence() {
        let mut ids: Vec<SensorId> = (0..20)
            .map(|i| SensorId {
                source: "cpu".into(),
                chip: "freq".into(),
                sensor: format!("cpu{i}"),
            })
            .collect();
        ids.reverse();
        ids.sort_by(|a, b| a.natural_cmp(b));
        let names: Vec<&str> = ids.iter().map(|id| id.sensor.as_str()).collect();
        assert_eq!(names[0], "cpu0");
        assert_eq!(names[1], "cpu1");
        assert_eq!(names[9], "cpu9");
        assert_eq!(names[10], "cpu10");
        assert_eq!(names[19], "cpu19");
    }

    #[test]
    fn test_sensor_unit_display() {
        assert_eq!(format!("{}", SensorUnit::Celsius), "\u{00b0}C");
        assert_eq!(format!("{}", SensorUnit::Watts), "W");
        assert_eq!(format!("{}", SensorUnit::Rpm), "RPM");
        assert_eq!(format!("{}", SensorUnit::Percent), "%");
    }
}
