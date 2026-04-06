use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::model::sensor::{SensorId, SensorReading};

#[derive(Debug, Clone)]
pub struct AlertRule {
    pub sensor_pattern: String,
    pub threshold: f64,
    pub direction: AlertDirection,
    pub cooldown: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AlertDirection {
    Above,
    Below,
}

pub struct AlertEngine {
    rules: Vec<AlertRule>,
    last_triggered: HashMap<String, Instant>,
}

impl AlertEngine {
    pub fn new(rules: Vec<AlertRule>) -> Self {
        Self {
            rules,
            last_triggered: HashMap::new(),
        }
    }

    /// Check all rules against current sensor readings.
    /// Returns a list of alert messages for newly triggered alerts.
    pub fn check(&mut self, readings: &HashMap<SensorId, SensorReading>) -> Vec<String> {
        let mut messages = Vec::new();
        let now = Instant::now();

        for rule in &self.rules {
            for (id, reading) in readings {
                let id_str = id.to_string();
                if !matches_pattern(&id_str, &rule.sensor_pattern) {
                    continue;
                }

                let triggered = match rule.direction {
                    AlertDirection::Above => reading.current > rule.threshold,
                    AlertDirection::Below => reading.current < rule.threshold,
                };

                if !triggered {
                    continue;
                }

                // Check cooldown
                let key = format!("{}:{}", rule.sensor_pattern, id_str);
                if let Some(last) = self.last_triggered.get(&key)
                    && now.duration_since(*last) < rule.cooldown
                {
                    continue;
                }

                self.last_triggered.insert(key, now);

                let dir_str = match rule.direction {
                    AlertDirection::Above => "above",
                    AlertDirection::Below => "below",
                };
                messages.push(format!(
                    "ALERT: {} = {:.1} {} ({} threshold {:.1})",
                    reading.label, reading.current, reading.unit, dir_str, rule.threshold
                ));
            }
        }

        messages
    }
}

/// Simple pattern matching: exact match or glob-style suffix match with *.
/// "hwmon/nct6798/temp*" matches "hwmon/nct6798/temp1", "hwmon/nct6798/temp2", etc.
/// "nvml/gpu0/temperature" matches exactly.
fn matches_pattern(id: &str, pattern: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix('*') {
        id.starts_with(prefix)
    } else {
        id == pattern
    }
}

/// Parse alert rules from CLI-style strings.
/// Format: "sensor_pattern > threshold" or "sensor_pattern < threshold"
/// Example: "hwmon/nct6798/temp1 > 80"
/// Optional cooldown suffix: "hwmon/nct6798/temp1 > 80 @60s"
pub fn parse_alert_rule(s: &str) -> Option<AlertRule> {
    let s = s.trim();

    let (rest, cooldown) = if let Some((before, after)) = s.rsplit_once('@') {
        let secs = after
            .trim()
            .strip_suffix('s')
            .unwrap_or(after.trim())
            .parse::<u64>()
            .ok()?;
        (before.trim(), Duration::from_secs(secs))
    } else {
        (s, Duration::from_secs(30))
    };

    let (sensor_pattern, direction, threshold) =
        if let Some((sensor, thresh)) = rest.split_once('>') {
            (
                sensor.trim().to_string(),
                AlertDirection::Above,
                thresh.trim().parse::<f64>().ok()?,
            )
        } else if let Some((sensor, thresh)) = rest.split_once('<') {
            (
                sensor.trim().to_string(),
                AlertDirection::Below,
                thresh.trim().parse::<f64>().ok()?,
            )
        } else {
            return None;
        };

    Some(AlertRule {
        sensor_pattern,
        threshold,
        direction,
        cooldown,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_alert_above() {
        let rule = parse_alert_rule("hwmon/nct6798/temp1 > 80").unwrap();
        assert_eq!(rule.sensor_pattern, "hwmon/nct6798/temp1");
        assert_eq!(rule.threshold, 80.0);
        assert_eq!(rule.direction, AlertDirection::Above);
        assert_eq!(rule.cooldown, Duration::from_secs(30));
    }

    #[test]
    fn test_parse_alert_below_with_cooldown() {
        let rule = parse_alert_rule("hwmon/nct6798/fan1 < 100 @10s").unwrap();
        assert_eq!(rule.sensor_pattern, "hwmon/nct6798/fan1");
        assert_eq!(rule.threshold, 100.0);
        assert_eq!(rule.direction, AlertDirection::Below);
        assert_eq!(rule.cooldown, Duration::from_secs(10));
    }

    #[test]
    fn test_parse_alert_invalid() {
        assert!(parse_alert_rule("no operator here").is_none());
        assert!(parse_alert_rule("sensor > notanumber").is_none());
    }

    #[test]
    fn test_matches_pattern_exact() {
        assert!(matches_pattern(
            "hwmon/nct6798/temp1",
            "hwmon/nct6798/temp1"
        ));
        assert!(!matches_pattern(
            "hwmon/nct6798/temp1",
            "hwmon/nct6798/temp2"
        ));
    }

    #[test]
    fn test_matches_pattern_glob() {
        assert!(matches_pattern(
            "hwmon/nct6798/temp1",
            "hwmon/nct6798/temp*"
        ));
        assert!(matches_pattern(
            "hwmon/nct6798/temp12",
            "hwmon/nct6798/temp*"
        ));
        assert!(!matches_pattern(
            "hwmon/k10temp/temp1",
            "hwmon/nct6798/temp*"
        ));
    }

    #[test]
    fn test_alert_engine_triggers() {
        let rules = vec![AlertRule {
            sensor_pattern: "test/chip/sensor".into(),
            threshold: 80.0,
            direction: AlertDirection::Above,
            cooldown: Duration::from_secs(0),
        }];
        let mut engine = AlertEngine::new(rules);

        let id = SensorId {
            source: "test".into(),
            chip: "chip".into(),
            sensor: "sensor".into(),
        };
        let reading = SensorReading::new(
            "Test Sensor".into(),
            85.0,
            crate::model::sensor::SensorUnit::Celsius,
            crate::model::sensor::SensorCategory::Temperature,
        );

        let mut readings = HashMap::new();
        readings.insert(id, reading);

        let alerts = engine.check(&readings);
        assert_eq!(alerts.len(), 1);
        assert!(alerts[0].contains("ALERT"));
        assert!(alerts[0].contains("85.0"));
    }

    #[test]
    fn test_alert_engine_respects_cooldown() {
        let rules = vec![AlertRule {
            sensor_pattern: "test/chip/sensor".into(),
            threshold: 80.0,
            direction: AlertDirection::Above,
            cooldown: Duration::from_secs(300),
        }];
        let mut engine = AlertEngine::new(rules);

        let id = SensorId {
            source: "test".into(),
            chip: "chip".into(),
            sensor: "sensor".into(),
        };
        let reading = SensorReading::new(
            "Test".into(),
            85.0,
            crate::model::sensor::SensorUnit::Celsius,
            crate::model::sensor::SensorCategory::Temperature,
        );

        let mut readings = HashMap::new();
        readings.insert(id, reading);

        let alerts1 = engine.check(&readings);
        assert_eq!(alerts1.len(), 1);

        // Second check within cooldown should not trigger
        let alerts2 = engine.check(&readings);
        assert_eq!(alerts2.len(), 0);
    }
}
