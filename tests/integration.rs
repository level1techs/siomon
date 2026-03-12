//! Integration tests for siomon.
//!
//! Tests CLI parsing, config merging, output formatting, and model operations
//! with known data. These run without root and without real hardware access.

use std::collections::HashMap;

// ── CLI + Config integration ────────────────────────────────────────────

#[test]
fn test_cli_default_format_is_text() {
    use clap::{CommandFactory, FromArgMatches};
    let matches = siomon::cli::Cli::command().get_matches_from(["sio"]);
    let cli = siomon::cli::Cli::from_arg_matches(&matches).unwrap();
    assert_eq!(cli.format, siomon::cli::OutputFormat::Text);
}

#[test]
fn test_cli_explicit_format_overrides_config() {
    use clap::{CommandFactory, FromArgMatches};
    let matches = siomon::cli::Cli::command().get_matches_from(["sio", "-f", "json"]);
    let mut cli = siomon::cli::Cli::from_arg_matches(&matches).unwrap();

    // Config says "xml", but CLI explicitly said "json"
    let config = siomon::config::SiomonConfig {
        general: siomon::config::GeneralConfig {
            format: "xml".into(),
            ..Default::default()
        },
        ..Default::default()
    };
    cli.apply_config(&config, &matches);
    assert_eq!(cli.format, siomon::cli::OutputFormat::Json);
}

#[test]
fn test_cli_config_applied_when_no_explicit_flag() {
    use clap::{CommandFactory, FromArgMatches};
    let matches = siomon::cli::Cli::command().get_matches_from(["sio"]);
    let mut cli = siomon::cli::Cli::from_arg_matches(&matches).unwrap();

    let config = siomon::config::SiomonConfig {
        general: siomon::config::GeneralConfig {
            format: "json".into(),
            poll_interval_ms: 500,
            no_nvidia: true,
            color: "never".into(),
            ..Default::default()
        },
        ..Default::default()
    };
    cli.apply_config(&config, &matches);

    assert_eq!(cli.format, siomon::cli::OutputFormat::Json);
    assert_eq!(cli.interval, 500);
    assert!(cli.no_nvidia);
    assert!(matches!(cli.color, siomon::cli::ColorMode::Never));
}

#[test]
fn test_cli_subcommand_parsing() {
    use clap::{CommandFactory, FromArgMatches};
    let matches = siomon::cli::Cli::command().get_matches_from(["sio", "cpu"]);
    let cli = siomon::cli::Cli::from_arg_matches(&matches).unwrap();
    assert!(matches!(cli.command, Some(siomon::cli::Commands::Cpu)));
}

#[test]
fn test_cli_monitor_mode() {
    use clap::{CommandFactory, FromArgMatches};
    let matches = siomon::cli::Cli::command().get_matches_from(["sio", "-m", "--interval", "2000"]);
    let cli = siomon::cli::Cli::from_arg_matches(&matches).unwrap();
    assert!(cli.tui);
    assert_eq!(cli.interval, 2000);
}

#[test]
fn test_cli_options_after_subcommand_works() {
    // Regression test for Issue #2 / PR #3
    use clap::{CommandFactory, FromArgMatches};
    // Format after subcommand should work now that format is global
    let matches = siomon::cli::Cli::command().get_matches_from(["sio", "sensors", "-f", "json"]);
    let cli = siomon::cli::Cli::from_arg_matches(&matches).unwrap();
    assert_eq!(cli.format, siomon::cli::OutputFormat::Json);
    assert!(matches!(cli.command, Some(siomon::cli::Commands::Sensors)));

    // Interval after subcommand
    let matches = siomon::cli::Cli::command().get_matches_from(["sio", "cpu", "--interval", "500"]);
    let cli = siomon::cli::Cli::from_arg_matches(&matches).unwrap();
    assert_eq!(cli.interval, 500);
}

#[test]
fn test_cli_flag_precedence_with_subcommand() {
    // Regression test for maintainer feedback
    use clap::{CommandFactory, FromArgMatches};
    let matches = siomon::cli::Cli::command().get_matches_from(["sio", "sensors", "-f", "json"]);
    let mut cli = siomon::cli::Cli::from_arg_matches(&matches).unwrap();

    let config = siomon::config::SiomonConfig {
        general: siomon::config::GeneralConfig {
            format: "xml".into(),
            ..Default::default()
        },
        ..Default::default()
    };

    cli.apply_config(&config, &matches);
    assert_eq!(cli.format, siomon::cli::OutputFormat::Json, "CLI flag -f json should take precedence");
}

#[test]
fn test_cli_no_flag_uses_config() {
    use clap::{CommandFactory, FromArgMatches};
    let matches = siomon::cli::Cli::command().get_matches_from(["sio", "sensors"]);
    let mut cli = siomon::cli::Cli::from_arg_matches(&matches).unwrap();

    let config = siomon::config::SiomonConfig {
        general: siomon::config::GeneralConfig {
            format: "xml".into(),
            ..Default::default()
        },
        ..Default::default()
    };

    cli.apply_config(&config, &matches);
    assert_eq!(cli.format, siomon::cli::OutputFormat::Xml, "No flag should use config value");
}

// ── Config parsing ──────────────────────────────────────────────────────

#[test]
fn test_config_roundtrip_toml() {
    let toml_str = r#"
[general]
format = "json"
poll_interval_ms = 750
no_nvidia = true
color = "always"

[sensor_labels]
"hwmon/nct6798/in0" = "Vcore"
"hwmon/nct6798/fan1" = "CPU Fan"
"#;
    let cfg: siomon::config::SiomonConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(cfg.general.format, "json");
    assert_eq!(cfg.general.poll_interval_ms, 750);
    assert!(cfg.general.no_nvidia);
    assert_eq!(cfg.general.color, "always");
    assert_eq!(cfg.sensor_labels.len(), 2);
    assert_eq!(cfg.sensor_labels["hwmon/nct6798/in0"], "Vcore");
}

// ── Sensor model operations ─────────────────────────────────────────────

#[test]
fn test_sensor_id_display_format() {
    let id = siomon::model::sensor::SensorId {
        source: "hwmon".into(),
        chip: "nct6798".into(),
        sensor: "in0".into(),
    };
    assert_eq!(id.to_string(), "hwmon/nct6798/in0");
}

#[test]
fn test_sensor_reading_update_tracks_min_max() {
    let mut reading = siomon::model::sensor::SensorReading::new(
        "Test".into(),
        25.0,
        siomon::model::sensor::SensorUnit::Celsius,
        siomon::model::sensor::SensorCategory::Temperature,
    );
    assert_eq!(reading.current, 25.0);
    assert_eq!(reading.min, 25.0);
    assert_eq!(reading.max, 25.0);

    reading.update(30.0);
    assert_eq!(reading.current, 30.0);
    assert_eq!(reading.min, 25.0);
    assert_eq!(reading.max, 30.0);

    reading.update(20.0);
    assert_eq!(reading.current, 20.0);
    assert_eq!(reading.min, 20.0);
    assert_eq!(reading.max, 30.0);
}

#[test]
fn test_sensor_id_natural_sort() {
    use std::cmp::Ordering;

    let id1 = siomon::model::sensor::SensorId {
        source: "hwmon".into(),
        chip: "nct6798".into(),
        sensor: "fan2".into(),
    };
    let id2 = siomon::model::sensor::SensorId {
        source: "hwmon".into(),
        chip: "nct6798".into(),
        sensor: "fan10".into(),
    };
    // Natural sort: fan2 < fan10
    assert_eq!(id1.natural_cmp(&id2), Ordering::Less);
}

// ── Sensor label loading ────────────────────────────────────────────────

#[test]
fn test_label_loading_with_config_override() {
    let mut user_labels = HashMap::new();
    user_labels.insert("hwmon/nct6798/in0".into(), "My Vcore".into());

    let labels = siomon::db::sensor_labels::load_labels(Some("WRX90E-SAGE SE"), &user_labels);

    // User label overrides builtin
    assert_eq!(labels["hwmon/nct6798/in0"], "My Vcore");
    // Builtin labels still present for other sensors
    assert_eq!(labels["hwmon/nct6798/fan1"], "CPU Fan");
}

#[test]
fn test_label_loading_no_board_user_only() {
    let mut user_labels = HashMap::new();
    user_labels.insert("hwmon/coretemp/temp1".into(), "CPU Package".into());

    let labels = siomon::db::sensor_labels::load_labels(None, &user_labels);
    assert_eq!(labels.len(), 1);
    assert_eq!(labels["hwmon/coretemp/temp1"], "CPU Package");
}

// ── Voltage scaling ─────────────────────────────────────────────────────

#[test]
fn test_voltage_scaling_lookup_strix() {
    let config = siomon::db::voltage_scaling::lookup_nct6798(Some("ROG STRIX X670E-E GAMING WIFI"));
    assert!(config.is_some());
    let channels = config.unwrap();
    assert_eq!(channels[0].label, "Vcore");
    assert_eq!(channels[1].label, "+5V");
    assert_eq!(channels[1].multiplier, 5.0);
    assert_eq!(channels[4].label, "+12V");
    assert_eq!(channels[4].multiplier, 12.0);
}

#[test]
fn test_voltage_scaling_default_no_multipliers() {
    let def = siomon::db::voltage_scaling::default_nct6798();
    for ch in def.iter() {
        assert_eq!(
            ch.multiplier, 1.0,
            "Default channel {} should have multiplier 1.0",
            ch.label
        );
    }
}

// ── Storage model ───────────────────────────────────────────────────────

#[test]
fn test_storage_interface_serialization() {
    let iface = siomon::model::storage::StorageInterface::NVMe;
    let json = serde_json::to_string(&iface).unwrap();
    assert_eq!(json, "\"NVMe\"");

    let sata = siomon::model::storage::StorageInterface::SATA;
    let json = serde_json::to_string(&sata).unwrap();
    assert_eq!(json, "\"SATA\"");
}

// ── SATA SMART attribute parsing ────────────────────────────────────────

#[test]
fn test_sata_smart_temperature_mapping() {
    let smart = siomon::model::storage::SmartData {
        temperature_celsius: 42,
        available_spare_pct: 0,
        available_spare_threshold_pct: 0,
        percentage_used: 0,
        data_units_read: 0,
        data_units_written: 0,
        host_read_commands: 0,
        host_write_commands: 0,
        controller_busy_time_minutes: 0,
        power_cycles: 1500,
        power_on_hours: 8760,
        unsafe_shutdowns: 0,
        media_errors: 0,
        num_error_log_entries: 0,
        warning_composite_temp_time_minutes: 0,
        critical_composite_temp_time_minutes: 0,
        critical_warning: 0,
        total_bytes_read: 0,
        total_bytes_written: 0,
    };
    assert_eq!(smart.temperature_celsius, 42);
    assert_eq!(smart.power_cycles, 1500);
    assert_eq!(smart.power_on_hours, 8760);
}

// ── JSON output ─────────────────────────────────────────────────────────

#[test]
fn test_system_info_json_roundtrip() {
    let info = siomon::model::system::SystemInfo {
        timestamp: chrono::Utc::now(),
        version: "0.0.1-test".into(),
        hostname: "testhost".into(),
        kernel_version: "6.14.0".into(),
        os_name: Some("Test Linux".into()),
        cpus: Vec::new(),
        memory: Default::default(),
        motherboard: Default::default(),
        gpus: Vec::new(),
        storage: Vec::new(),
        network: Vec::new(),
        audio: Vec::new(),
        usb_devices: Vec::new(),
        pci_devices: Vec::new(),
        batteries: Vec::new(),
        sensors: None,
    };

    let json = serde_json::to_string_pretty(&info).unwrap();
    assert!(json.contains("\"hostname\": \"testhost\""));
    assert!(json.contains("\"kernel_version\": \"6.14.0\""));

    // Roundtrip
    let parsed: siomon::model::system::SystemInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.hostname, "testhost");
    assert_eq!(parsed.kernel_version, "6.14.0");
}
