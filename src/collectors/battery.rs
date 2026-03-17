use crate::model::battery::BatteryInfo;
#[cfg(any(unix, windows))]
use crate::model::battery::{BatteryChemistry, BatteryStatus};
#[cfg(unix)]
use crate::platform::sysfs;
#[cfg(unix)]
use std::path::Path;

// ── Fallback for non-unix/non-windows platforms ────────────────────────────
#[cfg(not(any(unix, windows)))]
pub fn collect() -> Vec<BatteryInfo> {
    vec![]
}

// ── Windows (WMI via PowerShell) implementation ────────────────────────────

#[cfg(windows)]
pub fn collect() -> Vec<BatteryInfo> {
    collect_via_powershell().unwrap_or_default()
}

/// Query Win32_Battery for battery information.
#[cfg(windows)]
fn collect_via_powershell() -> Option<Vec<BatteryInfo>> {
    let ps_script = r#"
Get-CimInstance Win32_Battery | Select-Object Name,DeviceID,Manufacturer,Chemistry,BatteryStatus,EstimatedChargeRemaining,DesignCapacity,FullChargeCapacity,DesignVoltage | ConvertTo-Json -Compress
"#;

    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", ps_script])
        .output()
        .ok()?;

    if !output.status.success() {
        log::debug!(
            "Battery PowerShell query failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stdout = stdout.trim();
    if stdout.is_empty() {
        // No batteries (e.g. desktop) — return empty vec gracefully
        return Some(Vec::new());
    }

    // PowerShell ConvertTo-Json returns a bare object (not array) when there's
    // exactly one result. Normalise to always be an array.
    let json_str = if stdout.starts_with('[') {
        stdout.to_string()
    } else {
        format!("[{}]", stdout)
    };

    let raw: Vec<WmiBatteryRow> = serde_json::from_str(&json_str).ok()?;

    let devices: Vec<BatteryInfo> = raw.iter().filter_map(wmi_row_to_battery).collect();
    Some(devices)
}

#[cfg(windows)]
#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct WmiBatteryRow {
    #[serde(rename = "Name")]
    name: Option<String>,
    #[serde(rename = "DeviceID")]
    device_id: Option<String>,
    #[serde(rename = "Manufacturer")]
    manufacturer: Option<String>,
    #[serde(rename = "Chemistry")]
    chemistry: Option<u32>,
    #[serde(rename = "BatteryStatus")]
    battery_status: Option<u32>,
    #[serde(rename = "EstimatedChargeRemaining")]
    estimated_charge_remaining: Option<u64>,
    #[serde(rename = "DesignCapacity")]
    design_capacity: Option<u64>,
    #[serde(rename = "FullChargeCapacity")]
    full_charge_capacity: Option<u64>,
    #[serde(rename = "DesignVoltage")]
    design_voltage: Option<u64>,
}

#[cfg(windows)]
fn wmi_row_to_battery(row: &WmiBatteryRow) -> Option<BatteryInfo> {
    // Name falls back to DeviceID
    let name = row
        .name
        .clone()
        .or_else(|| row.device_id.clone())
        .unwrap_or_else(|| "Unknown Battery".to_string());

    let chemistry = row
        .chemistry
        .map(classify_chemistry_wmi)
        .unwrap_or(BatteryChemistry::Unknown("unknown".into()));

    let status = row
        .battery_status
        .map(classify_status_wmi)
        .unwrap_or(BatteryStatus::Unknown);

    let capacity_percent = row.estimated_charge_remaining.map(|v| v.min(100) as u8);

    // WMI reports DesignCapacity and FullChargeCapacity in mWh.
    // The model stores capacity in uWh (microwatt-hours).
    let design_capacity_uwh = row.design_capacity.map(|v| v * 1000);
    let full_charge_capacity_uwh = row.full_charge_capacity.map(|v| v * 1000);

    // WMI reports DesignVoltage in mV. Model stores voltage in uV.
    let voltage_now_uv = row.design_voltage.map(|v| v * 1000);

    let wear_percent = match (row.full_charge_capacity, row.design_capacity) {
        (Some(full), Some(design)) if design > 0 => Some(1.0 - (full as f64 / design as f64)),
        _ => None,
    };

    Some(BatteryInfo {
        name,
        manufacturer: row.manufacturer.clone(),
        model_name: row.name.clone(),
        chemistry,
        status,
        design_capacity_uwh,
        full_charge_capacity_uwh,
        remaining_capacity_uwh: None, // Not directly available from Win32_Battery
        voltage_now_uv,
        power_now_uw: None, // Not available from Win32_Battery
        capacity_percent,
        cycle_count: None, // Not available from Win32_Battery
        wear_percent,
    })
}

/// Map Win32_Battery Chemistry enum to BatteryChemistry.
/// WMI values: 1=Other, 2=Unknown, 3=Lead Acid, 4=Nickel Cadmium,
///             5=Nickel Metal Hydride, 6=Lithium-ion, 7=Zinc air, 8=Lithium Polymer
#[cfg(windows)]
fn classify_chemistry_wmi(code: u32) -> BatteryChemistry {
    match code {
        4 => BatteryChemistry::NickelCadmium,
        5 => BatteryChemistry::NickelMetalHydride,
        6 => BatteryChemistry::LithiumIon,
        8 => BatteryChemistry::LithiumPolymer,
        1 => BatteryChemistry::Unknown("Other".into()),
        3 => BatteryChemistry::Unknown("Lead Acid".into()),
        7 => BatteryChemistry::Unknown("Zinc Air".into()),
        _ => BatteryChemistry::Unknown("Unknown".into()),
    }
}

/// Map Win32_Battery BatteryStatus enum to BatteryStatus.
/// WMI values: 1=Discharging, 2=AC (Not Charging), 3=Fully Charged,
///             4=Low, 5=Critical, 6=Charging, 7=Charging and High,
///             8=Charging and Low, 9=Charging and Critical,
///             10=Undefined, 11=Partially Charged
#[cfg(windows)]
fn classify_status_wmi(code: u32) -> BatteryStatus {
    match code {
        1 | 4 | 5 => BatteryStatus::Discharging,
        2 => BatteryStatus::NotCharging,
        3 => BatteryStatus::Full,
        6..=9 => BatteryStatus::Charging,
        _ => BatteryStatus::Unknown,
    }
}

#[cfg(unix)]
pub fn collect() -> Vec<BatteryInfo> {
    let mut batteries = Vec::new();

    for entry in sysfs::glob_paths("/sys/class/power_supply/*") {
        let name = match entry.file_name() {
            Some(n) => n.to_string_lossy().to_string(),
            None => continue,
        };

        // Only process Battery-type power supplies
        let supply_type = sysfs::read_string_optional(&entry.join("type")).unwrap_or_default();
        if supply_type != "Battery" {
            continue;
        }

        if let Some(battery) = collect_battery(&name, &entry) {
            batteries.push(battery);
        }
    }

    batteries.sort_by(|a, b| a.name.cmp(&b.name));
    batteries
}

pub struct BatteryCollector;

impl crate::collectors::Collector for BatteryCollector {
    fn name(&self) -> &str {
        "battery"
    }

    fn collect_into(&self, info: &mut crate::model::system::SystemInfo) {
        info.batteries = collect();
    }
}

#[cfg(unix)]
fn collect_battery(name: &str, path: &Path) -> Option<BatteryInfo> {
    let manufacturer = sysfs::read_string_optional(&path.join("manufacturer"));
    let model_name = sysfs::read_string_optional(&path.join("model_name"));

    let chemistry = sysfs::read_string_optional(&path.join("technology"))
        .map(|s| classify_chemistry(&s))
        .unwrap_or(BatteryChemistry::Unknown("unknown".into()));

    let status = sysfs::read_string_optional(&path.join("status"))
        .map(|s| classify_status(&s))
        .unwrap_or(BatteryStatus::Unknown);

    let design_capacity_uwh = sysfs::read_u64_optional(&path.join("energy_full_design"));
    let full_charge_capacity_uwh = sysfs::read_u64_optional(&path.join("energy_full"));
    let remaining_capacity_uwh = sysfs::read_u64_optional(&path.join("energy_now"));
    let voltage_now_uv = sysfs::read_u64_optional(&path.join("voltage_now"));
    let power_now_uw = sysfs::read_u64_optional(&path.join("power_now"))
        .or_else(|| compute_power_from_current(path));

    let capacity_percent = sysfs::read_u64_optional(&path.join("capacity")).map(|v| v as u8);
    let cycle_count = sysfs::read_u32_optional(&path.join("cycle_count"));

    let wear_percent = match (full_charge_capacity_uwh, design_capacity_uwh) {
        (Some(full), Some(design)) if design > 0 => Some(1.0 - (full as f64 / design as f64)),
        _ => None,
    };

    Some(BatteryInfo {
        name: name.to_string(),
        manufacturer,
        model_name,
        chemistry,
        status,
        design_capacity_uwh,
        full_charge_capacity_uwh,
        remaining_capacity_uwh,
        voltage_now_uv,
        power_now_uw,
        capacity_percent,
        cycle_count,
        wear_percent,
    })
}

#[cfg(unix)]
fn compute_power_from_current(path: &Path) -> Option<u64> {
    let current_ua = sysfs::read_u64_optional(&path.join("current_now"))?;
    let voltage_uv = sysfs::read_u64_optional(&path.join("voltage_now"))?;
    // P = I * V; current_now is in uA, voltage_now is in uV
    // power in uW = (current_uA * voltage_uV) / 1_000_000
    Some(current_ua * voltage_uv / 1_000_000)
}

#[cfg(unix)]
fn classify_chemistry(technology: &str) -> BatteryChemistry {
    match technology {
        "Li-ion" => BatteryChemistry::LithiumIon,
        "Li-poly" => BatteryChemistry::LithiumPolymer,
        "NiMH" => BatteryChemistry::NickelMetalHydride,
        "NiCd" => BatteryChemistry::NickelCadmium,
        other => BatteryChemistry::Unknown(other.to_string()),
    }
}

#[cfg(unix)]
fn classify_status(status: &str) -> BatteryStatus {
    match status {
        "Charging" => BatteryStatus::Charging,
        "Discharging" => BatteryStatus::Discharging,
        "Full" => BatteryStatus::Full,
        "Not charging" => BatteryStatus::NotCharging,
        _ => BatteryStatus::Unknown,
    }
}
