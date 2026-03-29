use crate::db::boards::BoardTemplate;
use crate::model::memory::{DimmInfo, MemoryInfo, MemoryType};
use crate::parsers::smbios;
use crate::platform::procfs;

pub fn collect(direct_io: bool, board: Option<&'static BoardTemplate>) -> MemoryInfo {
    let meminfo = procfs::parse_meminfo();

    let total_bytes = meminfo.get("MemTotal").copied().unwrap_or(0);
    let available_bytes = meminfo.get("MemAvailable").copied().unwrap_or(0);
    let swap_total_bytes = meminfo.get("SwapTotal").copied().unwrap_or(0);
    let swap_free_bytes = meminfo.get("SwapFree").copied().unwrap_or(0);

    let mut dimms = collect_dimms();
    let populated_slots = if dimms.is_empty() {
        None
    } else {
        Some(dimms.len() as u32)
    };

    // Enrich DIMMs with SPD EEPROM data on supported boards (requires --direct-io).
    if let Some(b) = direct_io.then_some(board).flatten() {
        if let Some(ddr5_config) = b.ddr5_bus_config {
            enrich_dimms_with_spd(
                &mut dimms,
                ddr5_config,
                b.requirements.get(crate::db::boards::FEAT_DDR5),
            );
        }
    }

    MemoryInfo {
        total_bytes,
        available_bytes,
        swap_total_bytes,
        swap_free_bytes,
        max_capacity_bytes: None,
        total_slots: None,
        populated_slots,
        dimms,
    }
}

/// Read SPD EEPROMs and match them to SMBIOS DIMM entries by serial number.
fn enrich_dimms_with_spd(
    dimms: &mut [DimmInfo],
    ddr5_config: &crate::db::boards::Ddr5BusConfig,
    ddr5_requirements: &[crate::db::boards::Requirement],
) {
    let spd_results = super::spd::read_ddr5_spd(ddr5_config, ddr5_requirements);
    if spd_results.is_empty() {
        return;
    }

    log::info!(
        "SPD: enriching {} DIMM(s) with {} SPD result(s)",
        dimms.len(),
        spd_results.len()
    );

    for (spd_serial, spd_data) in spd_results {
        // Match by serial number (SMBIOS stores as ASCII, SPD as 4-byte hex).
        if let Some(dimm) = dimms.iter_mut().find(|d| {
            d.serial_number
                .as_ref()
                .is_some_and(|sn| sn.eq_ignore_ascii_case(&spd_serial))
        }) {
            log::debug!(
                "SPD: matched serial {} -> {} ({})",
                spd_serial,
                dimm.locator,
                dimm.bank_locator.as_deref().unwrap_or("?")
            );
            dimm.spd = Some(spd_data);
        } else {
            log::debug!("SPD: no SMBIOS match for serial {}", spd_serial);
        }
    }
}

fn collect_dimms() -> Vec<DimmInfo> {
    // Primary: parse the raw SMBIOS tables directly from sysfs.
    if let Some(smbios_data) = smbios::parse() {
        let dimms = convert_smbios_devices(&smbios_data.memory_devices);
        if !dimms.is_empty() {
            return dimms;
        }
    }

    // Fallback: shell out to dmidecode (requires root, may not be installed).
    collect_dimms_dmidecode()
}

/// Convert raw SMBIOS memory device entries into the model's DimmInfo.
fn convert_smbios_devices(devices: &[smbios::MemoryDeviceEntry]) -> Vec<DimmInfo> {
    devices
        .iter()
        .filter_map(|dev| {
            if dev.size_bytes == 0 {
                return None;
            }

            let memory_type = smbios_memory_type(dev.memory_type);
            let ecc = match (dev.total_width_bits, dev.data_width_bits) {
                (Some(tw), Some(dw)) => tw > dw,
                _ => false,
            };

            Some(DimmInfo {
                locator: dev.device_locator.clone().unwrap_or_default(),
                bank_locator: dev.bank_locator.clone(),
                manufacturer: dev.manufacturer.clone(),
                part_number: dev.part_number.clone(),
                serial_number: dev.serial_number.clone(),
                size_bytes: dev.size_bytes,
                memory_type,
                form_factor: dev.form_factor.clone(),
                type_detail: smbios::type_detail_string(dev.type_detail),
                configured_speed_mts: dev.configured_speed_mts,
                max_speed_mts: dev.speed_mts,
                configured_voltage_mv: dev.configured_voltage_mv.map(|v| v as u32),
                data_width_bits: dev.data_width_bits,
                total_width_bits: dev.total_width_bits,
                ecc,
                rank: dev.rank,
                spd: None,
            })
        })
        .collect()
}

/// Map the SMBIOS memory type byte to the model MemoryType enum.
fn smbios_memory_type(code: u8) -> MemoryType {
    match code {
        0x18 => MemoryType::DDR3,
        0x1A => MemoryType::DDR4,
        0x1E => MemoryType::LPDDR4,
        0x22 => MemoryType::DDR5,
        0x23 => MemoryType::LPDDR5,
        _ => MemoryType::Unknown(smbios::memory_type_name(code).to_string()),
    }
}

// ---------------------------------------------------------------------------
// dmidecode fallback (existing logic)
// ---------------------------------------------------------------------------

fn collect_dimms_dmidecode() -> Vec<DimmInfo> {
    let Ok(output) = std::process::Command::new("dmidecode")
        .args(["-t", "17"])
        .output()
    else {
        return Vec::new();
    };

    if !output.status.success() {
        return Vec::new();
    }

    let text = String::from_utf8_lossy(&output.stdout);
    parse_dmi_type17(&text)
}

fn parse_dmi_type17(text: &str) -> Vec<DimmInfo> {
    let mut dimms = Vec::new();
    let mut current: Option<DimmBuilder> = None;

    for line in text.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("Memory Device") {
            if let Some(builder) = current.take() {
                if let Some(dimm) = builder.build() {
                    dimms.push(dimm);
                }
            }
            current = Some(DimmBuilder::default());
        }

        if let Some(ref mut b) = current {
            if let Some((key, val)) = trimmed.split_once(':') {
                let key = key.trim();
                let val = val.trim();
                if val == "Not Provided"
                    || val == "Unknown"
                    || val == "No Module Installed"
                    || val == "Not Specified"
                {
                    continue;
                }
                match key {
                    "Locator" => b.locator = Some(val.to_string()),
                    "Bank Locator" => b.bank_locator = Some(val.to_string()),
                    "Manufacturer" => b.manufacturer = filter_placeholder(val),
                    "Part Number" => b.part_number = filter_placeholder(val),
                    "Serial Number" => b.serial_number = filter_placeholder(val),
                    "Size" => {
                        if let Some(mb_str) = val.strip_suffix(" MB") {
                            b.size_bytes =
                                mb_str.trim().parse::<u64>().ok().map(|v| v * 1024 * 1024);
                        } else if let Some(gb_str) = val.strip_suffix(" GB") {
                            b.size_bytes = gb_str
                                .trim()
                                .parse::<u64>()
                                .ok()
                                .map(|v| v * 1024 * 1024 * 1024);
                        }
                    }
                    "Type" => b.memory_type = Some(parse_memory_type(val)),
                    "Form Factor" => b.form_factor = Some(val.to_string()),
                    "Type Detail" => b.type_detail = Some(val.to_string()),
                    "Speed" => {
                        b.max_speed_mts = val
                            .split_whitespace()
                            .next()
                            .and_then(|s| s.parse::<u32>().ok());
                    }
                    "Configured Memory Speed" | "Configured Clock Speed" => {
                        b.configured_speed_mts = val
                            .split_whitespace()
                            .next()
                            .and_then(|s| s.parse::<u32>().ok());
                    }
                    "Configured Voltage" => {
                        if let Some(v_str) = val.strip_suffix(" V") {
                            b.configured_voltage_mv = v_str
                                .trim()
                                .parse::<f64>()
                                .ok()
                                .map(|v| (v * 1000.0) as u32);
                        }
                    }
                    "Data Width" => {
                        b.data_width_bits = val
                            .strip_suffix(" bits")
                            .and_then(|s| s.trim().parse::<u16>().ok());
                    }
                    "Total Width" => {
                        b.total_width_bits = val
                            .strip_suffix(" bits")
                            .and_then(|s| s.trim().parse::<u16>().ok());
                    }
                    "Rank" => b.rank = val.parse::<u8>().ok(),
                    _ => {}
                }
            }
        }
    }
    if let Some(builder) = current {
        if let Some(dimm) = builder.build() {
            dimms.push(dimm);
        }
    }
    dimms
}

fn filter_placeholder(val: &str) -> Option<String> {
    let v = val.trim();
    if v.is_empty()
        || v.chars().all(|c| c == '0' || c == ' ')
        || v == "Not Specified"
        || v == "Unknown"
    {
        None
    } else {
        Some(v.to_string())
    }
}

fn parse_memory_type(s: &str) -> MemoryType {
    match s {
        "DDR3" => MemoryType::DDR3,
        "DDR4" => MemoryType::DDR4,
        "DDR5" => MemoryType::DDR5,
        "LPDDR4" => MemoryType::LPDDR4,
        "LPDDR5" => MemoryType::LPDDR5,
        "LPDDR5X" => MemoryType::LPDDR5X,
        other => MemoryType::Unknown(other.to_string()),
    }
}

pub struct MemoryCollector {
    pub direct_io: bool,
    pub board: Option<&'static crate::db::boards::BoardTemplate>,
}

impl crate::collectors::Collector for MemoryCollector {
    fn name(&self) -> &str {
        "memory"
    }

    fn collect_into(&self, info: &mut crate::model::system::SystemInfo) {
        info.memory = collect(self.direct_io, self.board);
    }
}

#[derive(Default)]
struct DimmBuilder {
    locator: Option<String>,
    bank_locator: Option<String>,
    manufacturer: Option<String>,
    part_number: Option<String>,
    serial_number: Option<String>,
    size_bytes: Option<u64>,
    memory_type: Option<MemoryType>,
    form_factor: Option<String>,
    type_detail: Option<String>,
    configured_speed_mts: Option<u32>,
    max_speed_mts: Option<u32>,
    configured_voltage_mv: Option<u32>,
    data_width_bits: Option<u16>,
    total_width_bits: Option<u16>,
    rank: Option<u8>,
}

impl DimmBuilder {
    fn build(self) -> Option<DimmInfo> {
        let size = self.size_bytes?;
        if size == 0 {
            return None;
        }
        let ecc = match (self.total_width_bits, self.data_width_bits) {
            (Some(tw), Some(dw)) => tw > dw,
            _ => false,
        };
        Some(DimmInfo {
            locator: self.locator.unwrap_or_default(),
            bank_locator: self.bank_locator,
            manufacturer: self.manufacturer,
            part_number: self.part_number,
            serial_number: self.serial_number,
            size_bytes: size,
            memory_type: self
                .memory_type
                .unwrap_or(MemoryType::Unknown("Unknown".into())),
            form_factor: self.form_factor.unwrap_or_else(|| "Unknown".into()),
            type_detail: self.type_detail,
            configured_speed_mts: self.configured_speed_mts,
            max_speed_mts: self.max_speed_mts,
            configured_voltage_mv: self.configured_voltage_mv,
            data_width_bits: self.data_width_bits,
            total_width_bits: self.total_width_bits,
            ecc,
            rank: self.rank,
            spd: None,
        })
    }
}
