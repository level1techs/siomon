use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemoryInfo {
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub swap_total_bytes: u64,
    pub swap_free_bytes: u64,
    pub max_capacity_bytes: Option<u64>,
    pub total_slots: Option<u32>,
    pub populated_slots: Option<u32>,
    pub dimms: Vec<DimmInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DimmInfo {
    pub locator: String,
    pub bank_locator: Option<String>,
    pub manufacturer: Option<String>,
    pub part_number: Option<String>,
    pub serial_number: Option<String>,
    pub size_bytes: u64,
    pub memory_type: MemoryType,
    pub form_factor: String,
    pub type_detail: Option<String>,
    pub configured_speed_mts: Option<u32>,
    pub max_speed_mts: Option<u32>,
    pub configured_voltage_mv: Option<u32>,
    pub data_width_bits: Option<u16>,
    pub total_width_bits: Option<u16>,
    pub ecc: bool,
    pub rank: Option<u8>,
    /// SPD EEPROM data read directly from the DIMM, if available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spd: Option<SpdData>,
}

/// Parsed DDR5 SPD EEPROM data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpdData {
    /// SPD revision (e.g., "1.0").
    pub spd_revision: String,
    /// SDRAM density per die in gigabits.
    pub die_density_gb: u32,
    /// Number of die per package.
    pub die_per_package: u8,
    /// Number of bank groups.
    pub bank_groups: u8,
    /// Banks per bank group.
    pub banks_per_group: u8,
    /// Column address bits.
    pub column_bits: u8,
    /// Row address bits.
    pub row_bits: u8,
    /// Device (chip) I/O width in bits (x4, x8, x16).
    pub device_width: u8,
    /// Module type (RDIMM, UDIMM, LRDIMM, etc.).
    pub module_type: String,
    /// Minimum CAS latency time (tAA) in nanoseconds.
    pub t_aa_ns: Option<f64>,
    /// Minimum RAS-to-CAS delay (tRCD) in nanoseconds.
    pub t_rcd_ns: Option<f64>,
    /// Minimum row precharge time (tRP) in nanoseconds.
    pub t_rp_ns: Option<f64>,
    /// Minimum active-to-precharge time (tRAS) in nanoseconds.
    pub t_ras_ns: Option<f64>,
    /// Minimum row cycle time (tRC) in nanoseconds.
    pub t_rc_ns: Option<f64>,
    /// Supported CAS latencies.
    pub cas_latencies: Vec<u32>,
    /// SPD manufacturer name (from JEDEC bank/ID).
    pub spd_manufacturer: Option<String>,
    /// SPD part number (from EEPROM bytes 521–550).
    pub spd_part_number: Option<String>,
    /// I2C bus number where this SPD was read (for matching temperature sensors).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub i2c_bus: Option<u32>,
    /// I2C hub address where this SPD was read (0x50–0x57).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub i2c_addr: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MemoryType {
    DDR3,
    DDR4,
    DDR5,
    LPDDR4,
    LPDDR5,
    LPDDR5X,
    Unknown(String),
}
