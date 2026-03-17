//! Raw SMBIOS/DMI table parser.
//!
//! Reads the binary SMBIOS structures exposed by the kernel and extracts BIOS,
//! system, baseboard, and memory device information without shelling out to
//! `dmidecode`.
//!
//! On Linux the data comes from `/sys/firmware/dmi/tables/DMI`.
//! On Windows we call `GetSystemFirmwareTable('RSMB', 0, ...)` and skip the
//! 8-byte `RawSMBIOSData` header to get at the raw table bytes.

use std::path::Path;

// ---------------------------------------------------------------------------
// Public entry types
// ---------------------------------------------------------------------------

/// Parsed BIOS Information (SMBIOS Type 0).
#[derive(Debug, Clone)]
pub struct BiosEntry {
    pub vendor: Option<String>,
    pub version: Option<String>,
    pub release_date: Option<String>,
    pub major_release: Option<u8>,
    pub minor_release: Option<u8>,
}

/// Parsed System Information (SMBIOS Type 1).
#[derive(Debug, Clone)]
pub struct SystemEntry {
    pub manufacturer: Option<String>,
    pub product_name: Option<String>,
    pub uuid: Option<String>,
    pub sku_number: Option<String>,
    pub family: Option<String>,
}

/// Parsed Baseboard Information (SMBIOS Type 2).
#[derive(Debug, Clone)]
pub struct BaseboardEntry {
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub version: Option<String>,
    pub serial_number: Option<String>,
}

/// Parsed Physical Memory Array (SMBIOS Type 16).
#[derive(Debug, Clone)]
pub struct PhysicalMemoryArrayEntry {
    /// Maximum capacity in bytes.  `None` if unknown/not reported.
    pub max_capacity_bytes: Option<u64>,
    /// Number of memory device (Type 17) slots described by this array.
    pub number_of_devices: Option<u16>,
}

/// Parsed Memory Device (SMBIOS Type 17).
#[derive(Debug, Clone)]
pub struct MemoryDeviceEntry {
    pub total_width_bits: Option<u16>,
    pub data_width_bits: Option<u16>,
    pub size_bytes: u64,
    pub form_factor: String,
    pub device_locator: Option<String>,
    pub bank_locator: Option<String>,
    pub memory_type: u8,
    pub type_detail: u16,
    pub speed_mts: Option<u32>,
    pub manufacturer: Option<String>,
    pub serial_number: Option<String>,
    pub part_number: Option<String>,
    pub rank: Option<u8>,
    pub configured_speed_mts: Option<u32>,
    pub configured_voltage_mv: Option<u16>,
}

/// Top-level container for all parsed SMBIOS data.
#[derive(Debug, Clone)]
pub struct SmbiosData {
    pub bios: Option<BiosEntry>,
    pub system: Option<SystemEntry>,
    pub baseboard: Option<BaseboardEntry>,
    pub physical_memory_arrays: Vec<PhysicalMemoryArrayEntry>,
    pub memory_devices: Vec<MemoryDeviceEntry>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

#[cfg(unix)]
const DMI_TABLE_PATH: &str = "/sys/firmware/dmi/tables/DMI";

/// Parse the system's SMBIOS tables.
///
/// On Linux this reads from `/sys/firmware/dmi/tables/DMI`.
/// On Windows this calls `GetSystemFirmwareTable` with the `'RSMB'` provider.
///
/// Returns `None` if the table data cannot be obtained.
pub fn parse() -> Option<SmbiosData> {
    #[cfg(unix)]
    {
        parse_from_path(Path::new(DMI_TABLE_PATH))
    }

    #[cfg(windows)]
    {
        parse_windows()
    }
}

/// Parse SMBIOS structures from a DMI table file at an arbitrary path.
///
/// Useful for testing with fixture files.
pub fn parse_from_path(path: &Path) -> Option<SmbiosData> {
    let data = std::fs::read(path).ok()?;
    Some(parse_table(&data))
}

/// Parse SMBIOS structures from raw table bytes (no file header, no
/// `RawSMBIOSData` wrapper — just the packed SMBIOS structures).
pub fn parse_from_bytes(data: &[u8]) -> Option<SmbiosData> {
    if data.is_empty() {
        return None;
    }
    Some(parse_table(data))
}

// ---------------------------------------------------------------------------
// Windows: GetSystemFirmwareTable('RSMB')
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn parse_windows() -> Option<SmbiosData> {
    use winapi::um::sysinfoapi::GetSystemFirmwareTable;

    // 'RSMB' firmware table provider signature.
    // Windows multi-character literal packing: 'R'=0x52 in MSB → 0x52534D42.
    const RSMB: u32 = 0x5253_4D42;

    // First call: query required buffer size.
    let size = unsafe { GetSystemFirmwareTable(RSMB, 0, std::ptr::null_mut(), 0) };
    if size == 0 {
        log::warn!("SMBIOS: GetSystemFirmwareTable returned size 0");
        return None;
    }

    // Second call: fill the buffer.
    let mut buffer = vec![0u8; size as usize];
    let written = unsafe { GetSystemFirmwareTable(RSMB, 0, buffer.as_mut_ptr() as *mut _, size) };
    if written == 0 {
        log::warn!("SMBIOS: GetSystemFirmwareTable failed to fill buffer");
        return None;
    }

    // The returned data starts with an 8-byte RawSMBIOSData header:
    //   u8  Used20CallingMethod
    //   u8  SMBIOSMajorVersion
    //   u8  SMBIOSMinorVersion
    //   u8  DmiRevision
    //   u32 Length             (little-endian)
    //   [u8] SMBIOSTableData   — this is what we parse
    if buffer.len() < 8 {
        log::warn!("SMBIOS: buffer too small ({} bytes)", buffer.len());
        return None;
    }

    log::debug!(
        "SMBIOS: version {}.{}, table length {} bytes",
        buffer[1],
        buffer[2],
        u32::from_le_bytes([buffer[4], buffer[5], buffer[6], buffer[7]])
    );

    parse_from_bytes(&buffer[8..])
}

// ---------------------------------------------------------------------------
// Table walker
// ---------------------------------------------------------------------------

fn parse_table(data: &[u8]) -> SmbiosData {
    let mut result = SmbiosData {
        bios: None,
        system: None,
        baseboard: None,
        physical_memory_arrays: Vec::new(),
        memory_devices: Vec::new(),
    };

    let mut offset = 0;
    while offset + 4 <= data.len() {
        let struct_type = data[offset];
        let struct_len = data[offset + 1] as usize;

        // Minimum header length is 4 bytes.
        if struct_len < 4 {
            log::warn!(
                "SMBIOS: invalid structure length {} at offset {:#x}, stopping",
                struct_len,
                offset
            );
            break;
        }

        // Make sure the formatted area fits.
        if offset + struct_len > data.len() {
            break;
        }

        // The string section starts right after the formatted area and is
        // terminated by a double null (\0\0).  Find the end.
        let strings_start = offset + struct_len;
        let end = find_structure_end(data, strings_start);
        if end > data.len() {
            break;
        }

        let structure_data = &data[offset..end];

        // End-of-table marker (type 127).
        if struct_type == 127 {
            break;
        }

        match struct_type {
            0 => {
                if result.bios.is_none() {
                    result.bios = Some(parse_bios(structure_data, struct_len));
                }
            }
            1 => {
                if result.system.is_none() {
                    result.system = Some(parse_system(structure_data, struct_len));
                }
            }
            2 => {
                if result.baseboard.is_none() {
                    result.baseboard = Some(parse_baseboard(structure_data, struct_len));
                }
            }
            16 => {
                if let Some(arr) = parse_physical_memory_array(structure_data, struct_len) {
                    result.physical_memory_arrays.push(arr);
                }
            }
            17 => {
                if let Some(mem) = parse_memory_device(structure_data, struct_len) {
                    result.memory_devices.push(mem);
                }
            }
            _ => {}
        }

        offset = end;
    }

    result
}

/// Find the byte offset immediately after the double-null that terminates the
/// string section of an SMBIOS structure.
///
/// `strings_start` points to the first byte of the string section (right after
/// the formatted area).
fn find_structure_end(data: &[u8], strings_start: usize) -> usize {
    let mut pos = strings_start;

    // If the first two bytes are both null the structure has no strings.
    if pos + 1 < data.len() && data[pos] == 0 && data[pos + 1] == 0 {
        return pos + 2;
    }

    // Walk through null-terminated strings until we hit a double null.
    while pos < data.len() {
        if data[pos] == 0 {
            // This null terminates a string.  Check the next byte.
            if pos + 1 >= data.len() || data[pos + 1] == 0 {
                return pos + 2;
            }
        }
        pos += 1;
    }

    // Ran off the end of the buffer — return data.len() so the caller stops.
    data.len()
}

// ---------------------------------------------------------------------------
// String extraction helper
// ---------------------------------------------------------------------------

/// Extract the `index`-th (1-based) null-terminated string from the string
/// section that follows the formatted area of an SMBIOS structure.
///
/// `structure` is the full structure data (header + formatted area + strings).
/// `header_len` is the length field from the header (i.e. the formatted area
/// length including the 4-byte header).
///
/// Returns `None` for index 0 (meaning "no string") or if the index is out of
/// range.  Placeholder values commonly found in vendor firmware are also
/// filtered out to `None`.
pub fn get_string(structure: &[u8], header_len: u8, index: u8) -> Option<String> {
    if index == 0 {
        return None;
    }

    let start = header_len as usize;
    if start >= structure.len() {
        return None;
    }

    let string_area = &structure[start..];
    let mut current_index: u8 = 1;
    let mut pos = 0;

    while pos < string_area.len() {
        // Find the end of the current string.
        let end = string_area[pos..]
            .iter()
            .position(|&b| b == 0)
            .map(|p| pos + p)
            .unwrap_or(string_area.len());

        if current_index == index {
            let raw = &string_area[pos..end];
            let s = String::from_utf8_lossy(raw).trim().to_string();
            return filter_placeholder(&s);
        }

        current_index = current_index.saturating_add(1);
        pos = end + 1;

        // Double-null means end of string section.
        if pos < string_area.len() && string_area[pos] == 0 {
            break;
        }
    }

    None
}

/// Filter out common OEM placeholder / empty values.
fn filter_placeholder(s: &str) -> Option<String> {
    let v = s.trim();
    if v.is_empty()
        || v.chars().all(|c| c == '0' || c == ' ')
        || v == "Not Specified"
        || v == "Unknown"
        || v == "Not Provided"
        || v == "No Module Installed"
        || v == "To Be Filled By O.E.M."
        || v == "Default string"
        || v == "N/A"
    {
        None
    } else {
        Some(v.to_string())
    }
}

// ---------------------------------------------------------------------------
// Byte reading helpers
// ---------------------------------------------------------------------------

fn read_u8(data: &[u8], offset: usize) -> Option<u8> {
    data.get(offset).copied()
}

fn read_u16_le(data: &[u8], offset: usize) -> Option<u16> {
    if offset + 2 > data.len() {
        return None;
    }
    Some(u16::from_le_bytes([data[offset], data[offset + 1]]))
}

fn read_u32_le(data: &[u8], offset: usize) -> Option<u32> {
    if offset + 4 > data.len() {
        return None;
    }
    Some(u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ]))
}

/// Read a non-zero u16 value; returns `None` for 0 or 0xFFFF (unknown).
fn read_u16_nonzero(data: &[u8], offset: usize) -> Option<u16> {
    read_u16_le(data, offset).and_then(|v| if v == 0 || v == 0xFFFF { None } else { Some(v) })
}

// ---------------------------------------------------------------------------
// Type 0 — BIOS Information
// ---------------------------------------------------------------------------

fn parse_bios(data: &[u8], header_len: usize) -> BiosEntry {
    let hl = header_len as u8;
    let vendor = get_string(data, hl, read_u8(data, 0x04).unwrap_or(0));
    let version = get_string(data, hl, read_u8(data, 0x05).unwrap_or(0));
    let release_date = get_string(data, hl, read_u8(data, 0x08).unwrap_or(0));

    let major_release = if header_len > 0x12 {
        read_u8(data, 0x12)
    } else {
        None
    };
    let minor_release = if header_len > 0x13 {
        read_u8(data, 0x13)
    } else {
        None
    };

    BiosEntry {
        vendor,
        version,
        release_date,
        major_release,
        minor_release,
    }
}

// ---------------------------------------------------------------------------
// Type 1 — System Information
// ---------------------------------------------------------------------------

fn parse_system(data: &[u8], header_len: usize) -> SystemEntry {
    let hl = header_len as u8;
    let manufacturer = get_string(data, hl, read_u8(data, 0x04).unwrap_or(0));
    let product_name = get_string(data, hl, read_u8(data, 0x05).unwrap_or(0));

    // UUID is 16 bytes starting at offset 0x08.
    let uuid = if header_len >= 0x18 {
        format_uuid(&data[0x08..0x18])
    } else {
        None
    };

    let sku_number = if header_len > 0x19 {
        get_string(data, hl, read_u8(data, 0x19).unwrap_or(0))
    } else {
        None
    };
    let family = if header_len > 0x1A {
        get_string(data, hl, read_u8(data, 0x1A).unwrap_or(0))
    } else {
        None
    };

    SystemEntry {
        manufacturer,
        product_name,
        uuid,
        sku_number,
        family,
    }
}

/// Format a 16-byte SMBIOS UUID according to RFC 4122 with the mixed-endian
/// encoding used by SMBIOS 2.6+.
fn format_uuid(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 16 {
        return None;
    }

    // All 0xFF means "not present", all 0x00 means "not settable".
    if bytes.iter().all(|&b| b == 0xFF) || bytes.iter().all(|&b| b == 0x00) {
        return None;
    }

    // SMBIOS stores the first three fields in little-endian order.
    Some(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[3],
        bytes[2],
        bytes[1],
        bytes[0],
        bytes[5],
        bytes[4],
        bytes[7],
        bytes[6],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15],
    ))
}

// ---------------------------------------------------------------------------
// Type 2 — Baseboard Information
// ---------------------------------------------------------------------------

fn parse_baseboard(data: &[u8], header_len: usize) -> BaseboardEntry {
    let hl = header_len as u8;
    BaseboardEntry {
        manufacturer: get_string(data, hl, read_u8(data, 0x04).unwrap_or(0)),
        product: get_string(data, hl, read_u8(data, 0x05).unwrap_or(0)),
        version: get_string(data, hl, read_u8(data, 0x06).unwrap_or(0)),
        serial_number: get_string(data, hl, read_u8(data, 0x07).unwrap_or(0)),
    }
}

// ---------------------------------------------------------------------------
// Type 16 — Physical Memory Array
// ---------------------------------------------------------------------------

fn parse_physical_memory_array(data: &[u8], header_len: usize) -> Option<PhysicalMemoryArrayEntry> {
    // Minimum useful length: 0x0F bytes (covers Number of Memory Devices).
    if header_len < 0x0F {
        return None;
    }

    // Maximum Capacity at offset 0x07 (u32, kilobytes).
    // 0x80000000 means use Extended Maximum Capacity at offset 0x0F (u64, bytes).
    let max_capacity_bytes = {
        let raw_kb = read_u32_le(data, 0x07).unwrap_or(0);
        if raw_kb == 0x8000_0000 {
            // Extended Maximum Capacity at offset 0x0F (u64, bytes).
            if header_len > 0x16 {
                let lo = read_u32_le(data, 0x0F).unwrap_or(0) as u64;
                let hi = read_u32_le(data, 0x13).unwrap_or(0) as u64;
                let val = lo | (hi << 32);
                if val > 0 { Some(val) } else { None }
            } else {
                None
            }
        } else if raw_kb > 0 {
            Some(raw_kb as u64 * 1024)
        } else {
            None
        }
    };

    // Number of Memory Devices at offset 0x0D (u16).
    let number_of_devices =
        read_u16_le(data, 0x0D).and_then(|v| if v == 0 { None } else { Some(v) });

    Some(PhysicalMemoryArrayEntry {
        max_capacity_bytes,
        number_of_devices,
    })
}

// ---------------------------------------------------------------------------
// Type 17 — Memory Device
// ---------------------------------------------------------------------------

fn parse_memory_device(data: &[u8], header_len: usize) -> Option<MemoryDeviceEntry> {
    // Minimum length for a useful Type 17 structure.
    if header_len < 0x15 {
        return None;
    }

    let hl = header_len as u8;

    let total_width = read_u16_nonzero(data, 0x08);
    let data_width = read_u16_nonzero(data, 0x0A);

    // Size decoding (offset 0x0C).
    let raw_size = read_u16_le(data, 0x0C).unwrap_or(0);
    let size_bytes = decode_memory_size(raw_size, data, header_len);

    // Skip empty / not-installed slots.
    if size_bytes == 0 {
        return None;
    }

    let form_factor_byte = read_u8(data, 0x0E).unwrap_or(0);
    let form_factor = form_factor_name(form_factor_byte);

    let device_locator = get_string(data, hl, read_u8(data, 0x10).unwrap_or(0));
    let bank_locator = get_string(data, hl, read_u8(data, 0x11).unwrap_or(0));
    let memory_type = read_u8(data, 0x12).unwrap_or(0);
    let type_detail = read_u16_le(data, 0x13).unwrap_or(0);

    let speed_mts = if header_len > 0x16 {
        read_u16_nonzero(data, 0x15).map(|v| v as u32)
    } else {
        None
    };

    let manufacturer = if header_len > 0x17 {
        get_string(data, hl, read_u8(data, 0x17).unwrap_or(0))
    } else {
        None
    };
    let serial_number = if header_len > 0x18 {
        get_string(data, hl, read_u8(data, 0x18).unwrap_or(0))
    } else {
        None
    };
    let part_number = if header_len > 0x1A {
        get_string(data, hl, read_u8(data, 0x1A).unwrap_or(0))
    } else {
        None
    };

    let rank = if header_len > 0x1B {
        read_u8(data, 0x1B).and_then(|v| {
            let r = v & 0x0F;
            if r == 0 { None } else { Some(r) }
        })
    } else {
        None
    };

    let configured_speed_mts = if header_len > 0x21 {
        read_u16_nonzero(data, 0x20).map(|v| v as u32)
    } else {
        None
    };

    let configured_voltage_mv = if header_len > 0x27 {
        read_u16_nonzero(data, 0x26)
    } else {
        None
    };

    Some(MemoryDeviceEntry {
        total_width_bits: total_width,
        data_width_bits: data_width,
        size_bytes,
        form_factor,
        device_locator,
        bank_locator,
        memory_type,
        type_detail,
        speed_mts,
        manufacturer,
        serial_number,
        part_number,
        rank,
        configured_speed_mts,
        configured_voltage_mv,
    })
}

/// Decode the Size field from a Type 17 structure.
///
/// If raw_size is 0 the slot is empty.  If raw_size is 0x7FFF, the extended
/// size field (offset 0x1C) holds the real value in megabytes.  Otherwise
/// bit 15 selects the unit: 0 = megabytes, 1 = kilobytes.
fn decode_memory_size(raw_size: u16, data: &[u8], header_len: usize) -> u64 {
    if raw_size == 0 || raw_size == 0xFFFF {
        return 0;
    }

    if raw_size == 0x7FFF {
        // Extended size at offset 0x1C (u32, megabytes, bit 31 reserved).
        if header_len > 0x1F {
            if let Some(ext) = read_u32_le(data, 0x1C) {
                let mb = (ext & 0x7FFF_FFFF) as u64;
                return mb * 1024 * 1024;
            }
        }
        return 0;
    }

    let is_kb = raw_size & 0x8000 != 0;
    let value = (raw_size & 0x7FFF) as u64;
    if is_kb {
        value * 1024
    } else {
        value * 1024 * 1024
    }
}

// ---------------------------------------------------------------------------
// Lookup tables
// ---------------------------------------------------------------------------

/// Human-readable memory type name from the SMBIOS type byte.
pub fn memory_type_name(code: u8) -> &'static str {
    match code {
        0x01 => "Other",
        0x02 => "Unknown",
        0x03 => "DRAM",
        0x04 => "EDRAM",
        0x05 => "VRAM",
        0x06 => "SRAM",
        0x07 => "RAM",
        0x08 => "ROM",
        0x09 => "Flash",
        0x0A => "EEPROM",
        0x0B => "FEPROM",
        0x0C => "EPROM",
        0x0D => "CDRAM",
        0x0E => "3DRAM",
        0x0F => "SDRAM",
        0x10 => "SGRAM",
        0x11 => "RDRAM",
        0x12 => "DDR",
        0x13 => "DDR2",
        0x14 => "DDR2 FB-DIMM",
        0x18 => "DDR3",
        0x19 => "FBD2",
        0x1A => "DDR4",
        0x1B => "LPDDR",
        0x1C => "LPDDR2",
        0x1D => "LPDDR3",
        0x1E => "DDR5",
        0x1F => "LPDDR4",
        0x20 => "Logical non-volatile device",
        0x21 => "HBM",
        0x22 => "LPDDR5",
        0x23 => "HBM2",
        0x24 => "HBM3",
        0x25 => "LPDDR5X",
        _ => "Unknown",
    }
}

/// Human-readable form factor name.
fn form_factor_name(code: u8) -> String {
    match code {
        0x01 => "Other",
        0x02 => "Unknown",
        0x03 => "SIMM",
        0x04 => "SIP",
        0x05 => "Chip",
        0x06 => "DIP",
        0x07 => "ZIP",
        0x08 => "Proprietary Card",
        0x09 => "DIMM",
        0x0A => "TSOP",
        0x0B => "Row of chips",
        0x0C => "RIMM",
        0x0D => "SODIMM",
        0x0E => "SRIMM",
        0x0F => "FB-DIMM",
        0x10 => "Die",
        _ => "Unknown",
    }
    .to_string()
}

/// Decode the Type Detail bitmask into a comma-separated human-readable string.
pub fn type_detail_string(bits: u16) -> Option<String> {
    let names = [
        (1 << 1, "Other"),
        (1 << 2, "Unknown"),
        (1 << 3, "Fast-paged"),
        (1 << 4, "Static column"),
        (1 << 5, "Pseudo-static"),
        (1 << 6, "RAMBUS"),
        (1 << 7, "Synchronous"),
        (1 << 8, "CMOS"),
        (1 << 9, "EDO"),
        (1 << 10, "Window DRAM"),
        (1 << 11, "Cache DRAM"),
        (1 << 12, "Non-volatile"),
        (1 << 13, "Registered (Buffered)"),
        (1 << 14, "Unbuffered (Unregistered)"),
        (1 << 15, "LRDIMM"),
    ];

    let mut parts = Vec::new();
    for &(mask, name) in &names {
        if bits & mask != 0 {
            parts.push(name);
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal SMBIOS structure with the given type, formatted area,
    /// and strings.
    fn build_structure(stype: u8, formatted: &[u8], strings: &[&str]) -> Vec<u8> {
        let header_len = 4 + formatted.len();
        let mut buf = vec![stype, header_len as u8, 0x00, 0x00];
        // Formatted area
        buf.extend_from_slice(formatted);
        // String section
        if strings.is_empty() {
            buf.push(0x00);
            buf.push(0x00);
        } else {
            for s in strings {
                buf.extend_from_slice(s.as_bytes());
                buf.push(0x00);
            }
            buf.push(0x00); // double-null terminator
        }
        buf
    }

    /// Append an end-of-table marker (Type 127).
    fn append_eot(buf: &mut Vec<u8>) {
        buf.push(127); // type
        buf.push(4); // length
        buf.push(0x00);
        buf.push(0x00);
        buf.push(0x00);
        buf.push(0x00); // double-null
    }

    #[test]
    fn test_get_string_basic() {
        let structure = build_structure(0, &[0x01, 0x02], &["Hello", "World"]);
        assert_eq!(get_string(&structure, 6, 1), Some("Hello".to_string()));
        assert_eq!(get_string(&structure, 6, 2), Some("World".to_string()));
        assert_eq!(get_string(&structure, 6, 3), None);
        assert_eq!(get_string(&structure, 6, 0), None);
    }

    #[test]
    fn test_get_string_filters_placeholders() {
        let structure = build_structure(0, &[0x01], &["Not Specified"]);
        assert_eq!(get_string(&structure, 5, 1), None);

        let structure2 = build_structure(0, &[0x01], &["0000000000"]);
        assert_eq!(get_string(&structure2, 5, 1), None);
    }

    #[test]
    fn test_parse_bios() {
        // Type 0, formatted area large enough for major/minor release.
        // Offsets relative to structure start:
        // 0x04 = vendor string index = 1
        // 0x05 = version string index = 2
        // 0x06-0x07 = BIOS starting address (ignored)
        // 0x08 = release date string index = 3
        // 0x09 = ROM size byte = 0x0F  => (15+1)*64K = 1 MiB
        // 0x0A-0x11 = characteristics etc. (pad with zeros)
        // 0x12 = major release = 1
        // 0x13 = minor release = 29
        let mut formatted = vec![0u8; 0x14 - 4]; // 16 bytes
        formatted[0] = 1; // vendor string idx at offset 0x04 - 0x04 = 0
        formatted[1] = 2; // version at 0x05
        formatted[4] = 3; // release date at 0x08
        formatted[5] = 0x0F; // rom size at 0x09
        formatted[0x12 - 4] = 1; // major
        formatted[0x13 - 4] = 29; // minor

        let structure = build_structure(0, &formatted, &["ACME Corp", "v1.0", "01/01/2025"]);
        let header_len = 4 + formatted.len();
        let bios = parse_bios(&structure, header_len);

        assert_eq!(bios.vendor.as_deref(), Some("ACME Corp"));
        assert_eq!(bios.version.as_deref(), Some("v1.0"));
        assert_eq!(bios.release_date.as_deref(), Some("01/01/2025"));
        assert_eq!(bios.major_release, Some(1));
        assert_eq!(bios.minor_release, Some(29));
    }

    #[test]
    fn test_parse_system_uuid() {
        let uuid_bytes: [u8; 16] = [
            0x78, 0x56, 0x34, 0x12, // time-low LE
            0xBC, 0x9A, // time-mid LE
            0xF0, 0xDE, // time-hi LE
            0x01, 0x02, // clock-seq
            0x03, 0x04, 0x05, 0x06, 0x07, 0x08, // node
        ];

        // formatted area: offsets 0x04..0x1A relative to structure start
        // We need at least 0x1B - 4 = 0x17 bytes of formatted data.
        let mut formatted = vec![0u8; 0x1B - 4]; // 23 bytes
        formatted[0] = 1; // manufacturer string idx
        formatted[1] = 2; // product name
        formatted[2] = 0; // version (none)
        formatted[3] = 0; // serial (none)
        // UUID at offset 0x08 - 0x04 = 0x04
        formatted[4..20].copy_from_slice(&uuid_bytes);
        // 0x19 - 0x04 = 0x15
        formatted[0x15] = 3; // SKU string idx
        formatted[0x16] = 4; // Family string idx

        let structure = build_structure(
            1,
            &formatted,
            &["Test Vendor", "Test Product", "SKU-001", "Server"],
        );
        let header_len = 4 + formatted.len();
        let sys = parse_system(&structure, header_len);

        assert_eq!(sys.manufacturer.as_deref(), Some("Test Vendor"));
        assert_eq!(sys.product_name.as_deref(), Some("Test Product"));
        assert_eq!(
            sys.uuid.as_deref(),
            Some("12345678-9abc-def0-0102-030405060708")
        );
        assert_eq!(sys.sku_number.as_deref(), Some("SKU-001"));
        assert_eq!(sys.family.as_deref(), Some("Server"));
    }

    #[test]
    fn test_parse_baseboard() {
        let mut formatted = vec![0u8; 4]; // minimal: 4 string indices
        formatted[0] = 1; // manufacturer
        formatted[1] = 2; // product
        formatted[2] = 3; // version
        formatted[3] = 4; // serial

        let structure =
            build_structure(2, &formatted, &["BoardMfg", "BoardProd", "Rev1.0", "SN123"]);
        let header_len = 4 + formatted.len();
        let bb = parse_baseboard(&structure, header_len);

        assert_eq!(bb.manufacturer.as_deref(), Some("BoardMfg"));
        assert_eq!(bb.product.as_deref(), Some("BoardProd"));
        assert_eq!(bb.version.as_deref(), Some("Rev1.0"));
        assert_eq!(bb.serial_number.as_deref(), Some("SN123"));
    }

    #[test]
    fn test_parse_memory_device_basic() {
        // Build a Type 17 structure with enough fields.
        // header_len needs to cover through configured voltage (0x28 bytes total).
        let mut formatted = vec![0u8; 0x28 - 4]; // 36 bytes

        // Physical Memory Array Handle at 0x04 (offset 0 in formatted)
        formatted[0] = 0x01;
        formatted[1] = 0x00;
        // Total Width at 0x08 (offset 4)
        formatted[4] = 72;
        formatted[5] = 0; // 72 bits
        // Data Width at 0x0A (offset 6)
        formatted[6] = 64;
        formatted[7] = 0; // 64 bits
        // Size at 0x0C (offset 8) = 16384 MB = 16 GiB
        formatted[8] = 0x00;
        formatted[9] = 0x40; // 0x4000 = 16384
        // Form Factor at 0x0E (offset 10) = DIMM (0x09)
        formatted[10] = 0x09;
        // Device Locator at 0x10 (offset 12) = string 1
        formatted[12] = 1;
        // Bank Locator at 0x11 (offset 13) = string 2
        formatted[13] = 2;
        // Memory Type at 0x12 (offset 14) = DDR4 (0x1A)
        formatted[14] = 0x1A;
        // Type Detail at 0x13 (offset 15) = Synchronous | Unbuffered
        formatted[15] = 0x80; // bit 7 = Synchronous
        formatted[16] = 0x40; // bit 14 (in high byte) = Unbuffered
        // Speed at 0x15 (offset 17) = 3200 MT/s
        formatted[17] = 0x80;
        formatted[18] = 0x0C; // 0x0C80 = 3200
        // Manufacturer at 0x17 (offset 19) = string 3
        formatted[19] = 3;
        // Serial at 0x18 (offset 20) = string 4
        formatted[20] = 4;
        // Asset Tag at 0x19 (offset 21) = 0 (none)
        formatted[21] = 0;
        // Part Number at 0x1A (offset 22) = string 5
        formatted[22] = 5;
        // Attributes at 0x1B (offset 23) = 2 ranks
        formatted[23] = 2;
        // Extended Size at 0x1C (offset 24) = 0 (unused)
        // Configured Speed at 0x20 (offset 28) = 3200
        formatted[28] = 0x80;
        formatted[29] = 0x0C;
        // Min Voltage at 0x22 (offset 30) = 1200 mV
        formatted[30] = 0xB0;
        formatted[31] = 0x04; // 0x04B0 = 1200
        // Max Voltage at 0x24 (offset 32) = 1200 mV
        formatted[32] = 0xB0;
        formatted[33] = 0x04;
        // Configured Voltage at 0x26 (offset 34) = 1200 mV
        formatted[34] = 0xB0;
        formatted[35] = 0x04;

        let structure = build_structure(
            17,
            &formatted,
            &["DIMM_A1", "BANK 0", "Samsung", "SN-ABCD", "M393A2K43DB3"],
        );
        let header_len = 4 + formatted.len();
        let mem = parse_memory_device(&structure, header_len).unwrap();

        assert_eq!(mem.size_bytes, 16384 * 1024 * 1024);
        assert_eq!(mem.form_factor, "DIMM");
        assert_eq!(mem.device_locator.as_deref(), Some("DIMM_A1"));
        assert_eq!(mem.bank_locator.as_deref(), Some("BANK 0"));
        assert_eq!(mem.memory_type, 0x1A); // DDR4
        assert_eq!(mem.speed_mts, Some(3200));
        assert_eq!(mem.manufacturer.as_deref(), Some("Samsung"));
        assert_eq!(mem.serial_number.as_deref(), Some("SN-ABCD"));
        assert_eq!(mem.part_number.as_deref(), Some("M393A2K43DB3"));
        assert_eq!(mem.rank, Some(2));
        assert_eq!(mem.configured_speed_mts, Some(3200));
        assert_eq!(mem.configured_voltage_mv, Some(1200));
        assert_eq!(mem.total_width_bits, Some(72));
        assert_eq!(mem.data_width_bits, Some(64));
    }

    #[test]
    fn test_memory_size_kb_unit() {
        // Bit 15 set means kilobytes.
        let raw: u16 = 0x8000 | 512; // 512 KB
        assert_eq!(decode_memory_size(raw, &[], 0), 512 * 1024);
    }

    #[test]
    fn test_memory_size_extended() {
        // raw_size == 0x7FFF means use the extended field.
        let mut data = vec![0u8; 0x20];
        // Extended Size at offset 0x1C: 32768 MB = 32 GiB
        data[0x1C] = 0x00;
        data[0x1D] = 0x80;
        data[0x1E] = 0x00;
        data[0x1F] = 0x00; // 0x00008000 = 32768
        assert_eq!(
            decode_memory_size(0x7FFF, &data, 0x20),
            32768u64 * 1024 * 1024
        );
    }

    #[test]
    fn test_memory_size_empty() {
        assert_eq!(decode_memory_size(0, &[], 0), 0);
        assert_eq!(decode_memory_size(0xFFFF, &[], 0), 0);
    }

    #[test]
    fn test_parse_full_table() {
        let mut table = Vec::new();

        // Type 0 — BIOS
        {
            let mut formatted = vec![0u8; 0x14 - 4];
            formatted[0] = 1; // vendor
            formatted[1] = 2; // version
            formatted[4] = 3; // release date
            formatted[5] = 0x0F; // rom size
            formatted[0x12 - 4] = 1;
            formatted[0x13 - 4] = 5;
            table.extend_from_slice(&build_structure(
                0,
                &formatted,
                &["TestBIOS", "1.0.0", "12/25/2025"],
            ));
        }

        // Type 2 — Baseboard
        {
            let formatted = vec![1, 2, 3, 4]; // string indices
            table.extend_from_slice(&build_structure(
                2,
                &formatted,
                &["BoardVendor", "BoardModel", "v2", "BSN"],
            ));
        }

        // Type 17 — Memory (one populated slot)
        {
            let mut formatted = vec![0u8; 0x28 - 4];
            formatted[8] = 0x00; // size low
            formatted[9] = 0x20; // size high = 8192 MB = 8 GiB
            formatted[10] = 0x09; // DIMM
            formatted[12] = 1; // locator
            formatted[14] = 0x1E; // DDR5
            formatted[17] = 0xC0; // speed low
            formatted[18] = 0x12; // speed high = 0x12C0 = 4800
            formatted[19] = 2; // manufacturer
            formatted[22] = 3; // part number
            table.extend_from_slice(&build_structure(
                17,
                &formatted,
                &["DIMM_B1", "Micron", "MTC8C1084S1SC48BA1"],
            ));
        }

        // Type 17 — empty slot (size = 0)
        {
            let formatted = vec![0u8; 0x28 - 4];
            table.extend_from_slice(&build_structure(17, &formatted, &[]));
        }

        append_eot(&mut table);

        let result = parse_table(&table);

        assert!(result.bios.is_some());
        let bios = result.bios.unwrap();
        assert_eq!(bios.vendor.as_deref(), Some("TestBIOS"));
        assert_eq!(bios.major_release, Some(1));
        assert_eq!(bios.minor_release, Some(5));

        assert!(result.baseboard.is_some());
        let bb = result.baseboard.unwrap();
        assert_eq!(bb.manufacturer.as_deref(), Some("BoardVendor"));
        assert_eq!(bb.serial_number.as_deref(), Some("BSN"));

        // Only 1 populated device (empty slot filtered out).
        assert_eq!(result.memory_devices.len(), 1);
        let mem = &result.memory_devices[0];
        assert_eq!(mem.size_bytes, 8192u64 * 1024 * 1024);
        assert_eq!(mem.memory_type, 0x1E); // DDR5
        assert_eq!(mem.speed_mts, Some(4800));
        assert_eq!(mem.manufacturer.as_deref(), Some("Micron"));
    }

    #[test]
    fn test_type_detail_string() {
        // Synchronous + Unbuffered = bits 7 and 14
        let bits = (1 << 7) | (1 << 14);
        assert_eq!(
            type_detail_string(bits),
            Some("Synchronous, Unbuffered (Unregistered)".to_string())
        );

        assert_eq!(type_detail_string(0), None);
    }

    #[test]
    fn test_memory_type_name() {
        assert_eq!(memory_type_name(0x1A), "DDR4");
        assert_eq!(memory_type_name(0x1E), "DDR5");
        assert_eq!(memory_type_name(0x22), "LPDDR5");
        assert_eq!(memory_type_name(0x25), "LPDDR5X");
        assert_eq!(memory_type_name(0xFF), "Unknown");
    }

    #[test]
    fn test_format_uuid() {
        let bytes = [
            0x78, 0x56, 0x34, 0x12, 0xBC, 0x9A, 0xF0, 0xDE, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06,
            0x07, 0x08,
        ];
        assert_eq!(
            format_uuid(&bytes),
            Some("12345678-9abc-def0-0102-030405060708".to_string())
        );

        // All zeros.
        assert_eq!(format_uuid(&[0u8; 16]), None);
        // All 0xFF.
        assert_eq!(format_uuid(&[0xFF; 16]), None);
    }

    #[test]
    fn test_find_structure_end_no_strings() {
        // Double null right at the start of the string section.
        let data = [0x00, 0x04, 0x00, 0x00, 0x00, 0x00];
        assert_eq!(find_structure_end(&data, 4), 6);
    }

    #[test]
    fn test_find_structure_end_with_strings() {
        let mut data = vec![0x00, 0x06, 0x00, 0x00, 0x01, 0x02]; // header
        data.extend_from_slice(b"Hi\x00World\x00\x00");
        assert_eq!(find_structure_end(&data, 6), data.len());
    }
}
