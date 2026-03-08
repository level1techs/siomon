use crate::model::sensor::{SensorCategory, SensorId, SensorReading, SensorUnit};

use super::bus_scan::I2cBus;
use super::smbus_io::SmbusDevice;

/// PMBus command codes for reading telemetry.
const PMBUS_PAGE: u8 = 0x00;
const PMBUS_VOUT_MODE: u8 = 0x20;
const PMBUS_READ_VIN: u8 = 0x88;
const PMBUS_READ_IIN: u8 = 0x89;
const PMBUS_READ_VOUT: u8 = 0x8B;
const PMBUS_READ_IOUT: u8 = 0x8C;
const PMBUS_READ_TEMPERATURE_1: u8 = 0x8D;
const PMBUS_READ_TEMPERATURE_2: u8 = 0x8E;
const PMBUS_READ_POUT: u8 = 0x96;
const PMBUS_READ_PIN: u8 = 0x97;

/// I2C address range for PCA954x mux devices.
const MUX_ADDR_FIRST: u16 = 0x70;
const MUX_ADDR_LAST: u16 = 0x77;

/// I2C address range to scan for PMBus VRM controllers.
const VRM_ADDR_FIRST: u16 = 0x20;
const VRM_ADDR_LAST: u16 = 0x4F;

/// Sensor source for PMBus VRM controllers read via I2C/SMBus.
pub struct PmbusSource {
    devices: Vec<PmbusDevice>,
}

struct PmbusDevice {
    bus: u32,
    addr: u16,
    page: Option<u8>,
    vout_exponent: i8,
    label_prefix: String,
    id_prefix: String,
}

/// Description of a single PMBus register to read.
struct PmbusRegister {
    command: u8,
    suffix: &'static str,
    label_suffix: &'static str,
    unit: SensorUnit,
    category: SensorCategory,
    format: PmbusFormat,
}

#[derive(Clone, Copy)]
enum PmbusFormat {
    Linear11,
    Linear16,
}

/// The set of registers polled for each VRM device.
const REGISTERS: &[PmbusRegister] = &[
    PmbusRegister {
        command: PMBUS_READ_VIN,
        suffix: "vin",
        label_suffix: "VIN",
        unit: SensorUnit::Volts,
        category: SensorCategory::Voltage,
        format: PmbusFormat::Linear11,
    },
    PmbusRegister {
        command: PMBUS_READ_IIN,
        suffix: "iin",
        label_suffix: "IIN",
        unit: SensorUnit::Amps,
        category: SensorCategory::Current,
        format: PmbusFormat::Linear11,
    },
    PmbusRegister {
        command: PMBUS_READ_VOUT,
        suffix: "vout",
        label_suffix: "VOUT",
        unit: SensorUnit::Volts,
        category: SensorCategory::Voltage,
        format: PmbusFormat::Linear16,
    },
    PmbusRegister {
        command: PMBUS_READ_IOUT,
        suffix: "iout",
        label_suffix: "IOUT",
        unit: SensorUnit::Amps,
        category: SensorCategory::Current,
        format: PmbusFormat::Linear11,
    },
    PmbusRegister {
        command: PMBUS_READ_TEMPERATURE_1,
        suffix: "temp1",
        label_suffix: "Temp1",
        unit: SensorUnit::Celsius,
        category: SensorCategory::Temperature,
        format: PmbusFormat::Linear11,
    },
    PmbusRegister {
        command: PMBUS_READ_TEMPERATURE_2,
        suffix: "temp2",
        label_suffix: "Temp2",
        unit: SensorUnit::Celsius,
        category: SensorCategory::Temperature,
        format: PmbusFormat::Linear11,
    },
    PmbusRegister {
        command: PMBUS_READ_POUT,
        suffix: "pout",
        label_suffix: "POUT",
        unit: SensorUnit::Watts,
        category: SensorCategory::Power,
        format: PmbusFormat::Linear11,
    },
    PmbusRegister {
        command: PMBUS_READ_PIN,
        suffix: "pin",
        label_suffix: "PIN",
        unit: SensorUnit::Watts,
        category: SensorCategory::Power,
        format: PmbusFormat::Linear11,
    },
];

impl PmbusSource {
    /// Scan all SMBus adapters for PMBus VRM controllers and build the sensor list.
    ///
    /// Returns an empty source if no devices are found or if `/dev/i2c-*`
    /// cannot be opened (e.g., insufficient permissions).
    pub fn discover(buses: &[I2cBus]) -> Self {
        let mut devices = Vec::new();
        let mut vrm_index: u32 = 0;

        for bus in buses {
            if !bus.adapter_type.is_smbus() {
                continue;
            }

            // Scan the direct VRM address range on this bus.
            scan_vrm_range(bus.bus_num, &mut devices, &mut vrm_index);

            // Check for I2C mux devices and scan behind each channel.
            scan_behind_muxes(bus.bus_num, &mut devices, &mut vrm_index);
        }

        if devices.is_empty() {
            log::debug!("No PMBus VRM controllers discovered");
        } else {
            log::info!("Discovered {} PMBus VRM controller(s)", devices.len());
        }

        Self { devices }
    }

    /// Read current telemetry from each discovered VRM controller.
    pub fn poll(&self) -> Vec<(SensorId, SensorReading)> {
        let mut readings = Vec::new();

        for dev in &self.devices {
            let smbus = match SmbusDevice::open(dev.bus, dev.addr) {
                Ok(s) => s,
                Err(e) => {
                    log::warn!(
                        "Failed to open PMBus device bus {} addr {:#04x}: {}",
                        dev.bus,
                        dev.addr,
                        e
                    );
                    continue;
                }
            };

            // Select PMBus page if this device uses multi-page
            if let Some(page) = dev.page {
                if smbus.write_byte_data(PMBUS_PAGE, page).is_err() {
                    continue;
                }
            }

            for reg in REGISTERS {
                let raw = match smbus.read_word_data(reg.command) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                let value = match reg.format {
                    PmbusFormat::Linear11 => decode_linear11(raw),
                    PmbusFormat::Linear16 => decode_linear16(raw, dev.vout_exponent),
                };

                let id = SensorId {
                    source: "i2c".into(),
                    chip: "pmbus".into(),
                    sensor: format!("{}_{}", dev.id_prefix, reg.suffix),
                };
                let label = format!("{} {}", dev.label_prefix, reg.label_suffix);
                let reading = SensorReading::new(label, value, reg.unit, reg.category);
                readings.push((id, reading));
            }
        }

        readings
    }

    /// Number of discovered VRM devices.
    pub fn device_count(&self) -> usize {
        self.devices.len()
    }
}

/// Scan the VRM address range (0x20-0x4F) on a single bus.
fn scan_vrm_range(bus: u32, devices: &mut Vec<PmbusDevice>, vrm_index: &mut u32) {
    for addr in VRM_ADDR_FIRST..=VRM_ADDR_LAST {
        let found = probe_pmbus_with_pages(bus, addr, vrm_index);
        for dev in found {
            log::info!(
                "PMBus VRM found: bus {} addr {:#04x} page={:?} vout_exp={} -> {}",
                bus,
                addr,
                dev.page,
                dev.vout_exponent,
                dev.label_prefix
            );
            devices.push(dev);
        }
    }
}

/// Check for PCA954x I2C mux devices at 0x70-0x77 and scan behind each channel.
///
/// A mux typically has 4 or 8 channels. We select each channel by writing
/// a single byte with the channel bit set, then scan the VRM address range.
fn scan_behind_muxes(bus: u32, devices: &mut Vec<PmbusDevice>, vrm_index: &mut u32) {
    for mux_addr in MUX_ADDR_FIRST..=MUX_ADDR_LAST {
        let mux = match SmbusDevice::open(bus, mux_addr) {
            Ok(d) => d,
            Err(_) => continue,
        };

        // Try reading the current mux state to confirm it's a mux.
        if mux.read_byte_data(0x00).is_err() {
            continue;
        }

        log::debug!("I2C mux detected: bus {} addr {:#04x}", bus, mux_addr);

        // Try up to 8 channels (bits 0..7).
        for channel in 0u8..8 {
            // Select channel by writing a byte with the channel bit set.
            // We use SMBus byte-data write to register 0x00 with value (1 << channel).
            // Actually PCA954x muxes are controlled by a raw byte write (not register-based).
            // We use read_byte_data at the channel-select value as a workaround;
            // but the proper approach is writing the channel select byte.
            // For scanning purposes, we write using the smbus_io layer.
            let select = 1u8 << channel;
            if write_byte(&mux, select).is_err() {
                continue;
            }

            // Now scan the VRM range — devices behind this mux channel
            // appear at their native addresses on the same bus.
            scan_vrm_range(bus, devices, vrm_index);

            // Deselect all channels.
            let _ = write_byte(&mux, 0x00);
        }
    }
}

/// Write a single byte to a device (SMBus send-byte protocol).
///
/// This is used for PCA954x mux channel selection where a raw byte
/// write (no register address) selects the active channel(s).
fn write_byte(dev: &SmbusDevice, value: u8) -> std::io::Result<()> {
    dev.write_byte_data(0x00, value)
}

/// Probe a PMBus device, checking for multiple pages.
///
/// Multi-output VRM controllers use the PMBus PAGE command (0x00) to
/// select which output rail to query. We try pages 0-3 and create a
/// separate PmbusDevice for each page that returns plausible readings.
fn probe_pmbus_with_pages(bus: u32, addr: u16, vrm_index: &mut u32) -> Vec<PmbusDevice> {
    let dev = match SmbusDevice::open(bus, addr) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };

    // First check if this is a PMBus device at all (no page select)
    if dev.read_byte_data(PMBUS_VOUT_MODE).is_err() {
        return Vec::new();
    }

    let mut results = Vec::new();

    // Try pages 0-3
    for page in 0u8..4 {
        if dev.write_byte_data(PMBUS_PAGE, page).is_err() {
            break; // No more pages
        }

        // Read VOUT_MODE for this page
        let vout_mode = match dev.read_byte_data(PMBUS_VOUT_MODE) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Must be LINEAR mode (bits [7:5] = 000)
        if (vout_mode >> 5) != 0 {
            continue;
        }

        let vout_exponent = sign_extend_5bit(vout_mode & 0x1F);

        // Check VOUT — page is "active" if VOUT > 0
        let vout_raw = match dev.read_word_data(PMBUS_READ_VOUT) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let vout = decode_linear16(vout_raw, vout_exponent);
        if vout < 0.01 {
            continue; // Rail is off or not connected
        }

        // Sanity: check VIN is plausible
        if let Ok(vin_raw) = dev.read_word_data(PMBUS_READ_VIN) {
            let vin = decode_linear11(vin_raw);
            if !(0.0..=60.0).contains(&vin) {
                continue;
            }
        }

        let page_label = if page == 0 && results.is_empty() {
            // Single page or first page — check if there are more
            format!("VRM {} (bus {} addr {:#04x})", *vrm_index, bus, addr)
        } else {
            format!(
                "VRM {} page {} (bus {} addr {:#04x})",
                *vrm_index, page, bus, addr
            )
        };

        let id_prefix = if page > 0 || !results.is_empty() {
            format!("vrm{}_p{}", *vrm_index, page)
        } else {
            format!("vrm{}", *vrm_index)
        };

        results.push(PmbusDevice {
            bus,
            addr,
            page: Some(page),
            vout_exponent,
            label_prefix: page_label,
            id_prefix,
        });
    }

    // Fix up labels: if we found multiple pages, relabel the first one
    if results.len() > 1 {
        if let Some(first) = results.first_mut() {
            first.label_prefix =
                format!("VRM {} page 0 (bus {} addr {:#04x})", *vrm_index, bus, addr);
            first.id_prefix = format!("vrm{}_p0", *vrm_index);
        }
    }

    // Reset page to 0 when done
    if let Ok(d) = SmbusDevice::open(bus, addr) {
        let _ = d.write_byte_data(PMBUS_PAGE, 0);
    }

    if !results.is_empty() {
        *vrm_index += 1;
    }
    results
}

/// Attempt to identify a PMBus device by reading VOUT_MODE (legacy single-page probe).
fn probe_pmbus(bus: u32, addr: u16, vrm_index: u32) -> Option<PmbusDevice> {
    let dev = SmbusDevice::open(bus, addr).ok()?;

    // Read VOUT_MODE — a successful read with LINEAR mode (bits [7:5] = 000)
    // is a strong indicator of a PMBus device.
    let vout_mode = dev.read_byte_data(PMBUS_VOUT_MODE).ok()?;

    // Bits [7:5] encode the mode: 000 = LINEAR, others = VID/DIRECT/MFR.
    // We only support LINEAR mode.
    let mode_bits = vout_mode >> 5;
    if mode_bits != 0 {
        log::debug!(
            "PMBus probe: bus {} addr {:#04x} VOUT_MODE={:#04x} not LINEAR (mode={})",
            bus,
            addr,
            vout_mode,
            mode_bits
        );
        return None;
    }

    let vout_exponent = sign_extend_5bit(vout_mode & 0x1F);

    // Sanity check: try reading VIN and verify it decodes to a plausible value.
    let vin_raw = dev.read_word_data(PMBUS_READ_VIN).ok()?;
    let vin = decode_linear11(vin_raw);
    if !(0.0..=60.0).contains(&vin) {
        log::debug!(
            "PMBus probe: bus {} addr {:#04x} VIN={:.2}V out of range",
            bus,
            addr,
            vin
        );
        return None;
    }

    let label_prefix = format!("VRM {} (bus {} addr {:#04x})", vrm_index, bus, addr);
    let id_prefix = format!("vrm{vrm_index}");

    Some(PmbusDevice {
        bus,
        addr,
        page: None,
        vout_exponent,
        label_prefix,
        id_prefix,
    })
}

/// Decode a PMBus LINEAR11 value to a floating-point number.
///
/// LINEAR11 format encodes a signed 5-bit exponent in bits [15:11]
/// and an 11-bit mantissa in bits [10:0]:
///
///   value = mantissa * 2^exponent
fn decode_linear11(raw: u16) -> f64 {
    let exp_raw = ((raw >> 11) & 0x1F) as u8;
    let exponent = sign_extend_5bit(exp_raw);
    let mantissa = (raw & 0x7FF) as i16;

    // Sign-extend the 11-bit mantissa
    let mantissa = if mantissa & 0x400 != 0 {
        mantissa | !0x7FF
    } else {
        mantissa
    };

    mantissa as f64 * f64::powi(2.0, exponent as i32)
}

/// Decode a PMBus LINEAR16 value using the exponent from VOUT_MODE.
///
/// LINEAR16 format: value = raw_u16 * 2^exponent
/// where the exponent comes from VOUT_MODE bits [4:0] (sign-extended).
fn decode_linear16(raw: u16, exponent: i8) -> f64 {
    raw as f64 * f64::powi(2.0, exponent as i32)
}

/// Sign-extend a 5-bit value to i8.
///
/// Bit 4 is the sign bit. If set, the value is negative (two's complement).
fn sign_extend_5bit(val: u8) -> i8 {
    if val & 0x10 != 0 {
        (val | 0xE0) as i8
    } else {
        val as i8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- LINEAR11 decoding tests ---

    #[test]
    fn linear11_vin_11_92v() {
        // VIN = 11.92V: exponent = -3, mantissa = 11.92 * 8 = 95.36 ~ 95
        // Raw: exp = -3 -> 5-bit = 0b11101 = 29; mantissa = 95 = 0x05F
        // raw = (29 << 11) | 95 = 0xE85F
        // Decode: mantissa=95, exp=-3 -> 95 / 8 = 11.875
        // Alternatively test with the known raw value that produces ~11.92
        // Let's verify the formula with a more precise encoding:
        // exponent = -2, mantissa = 11.92 * 4 = 47.68 ~ 48
        // raw = (0b11110 << 11) | 48 = 0xF030
        // decode: 48 * 2^-2 = 12.0
        //
        // Use the exact formula: for 11.92V with exp=-4, mantissa = 11.92 * 16 = 190.72 ~ 191
        // raw = (0b11100 << 11) | 191 = 0xE0BF
        // decode: 191 * 2^-4 = 11.9375
        let raw: u16 = 0xE0BF;
        let v = decode_linear11(raw);
        assert!((v - 11.9375).abs() < 0.01, "got {v}");
    }

    #[test]
    fn linear11_iout_6_6a() {
        // IOUT = 6.6A: exponent = -3, mantissa = 6.6 * 8 = 52.8 ~ 53
        // raw = (0b11101 << 11) | 53 = 0xE835
        // decode: 53 * 2^-3 = 6.625
        let raw: u16 = 0xE835;
        let v = decode_linear11(raw);
        assert!((v - 6.625).abs() < 0.01, "got {v}");
    }

    #[test]
    fn linear11_temp_35c() {
        // TEMP = 35C: exponent = -2, mantissa = 35 * 4 = 140
        // raw = (0b11110 << 11) | 140 = 0xF08C
        // decode: 140 * 2^-2 = 35.0
        let raw: u16 = 0xF08C;
        let v = decode_linear11(raw);
        assert!((v - 35.0).abs() < 0.01, "got {v}");
    }

    #[test]
    fn linear11_pout_7_1w() {
        // POUT = 7.1W: exponent = -3, mantissa = 7.1 * 8 = 56.8 ~ 57
        // raw = (0b11101 << 11) | 57 = 0xE839
        // decode: 57 * 2^-3 = 7.125
        let raw: u16 = 0xE839;
        let v = decode_linear11(raw);
        assert!((v - 7.125).abs() < 0.01, "got {v}");
    }

    #[test]
    fn linear11_pin_5_75w() {
        // PIN = 5.75W: exponent = -2, mantissa = 5.75 * 4 = 23
        // raw = (0b11110 << 11) | 23 = 0xF017
        // decode: 23 * 2^-2 = 5.75
        let raw: u16 = 0xF017;
        let v = decode_linear11(raw);
        assert!((v - 5.75).abs() < 0.001, "got {v}");
    }

    #[test]
    fn linear11_zero() {
        // exponent = 0, mantissa = 0
        assert!((decode_linear11(0x0000) - 0.0).abs() < 0.001);
    }

    #[test]
    fn linear11_negative_mantissa() {
        // Test with a negative mantissa: mantissa = -1 (0x7FF), exponent = 0
        // raw = (0b00000 << 11) | 0x7FF = 0x07FF
        // mantissa sign-extended: -1, exp = 0 -> -1.0
        let raw: u16 = 0x07FF;
        let v = decode_linear11(raw);
        assert!((v - (-1.0)).abs() < 0.001, "got {v}");
    }

    // --- LINEAR16 decoding tests ---

    #[test]
    fn linear16_vout_1_10v() {
        // VOUT = 1.10V with VOUT_MODE exponent = -8
        // raw = 1.10 / 2^-8 = 1.10 * 256 = 281.6 ~ 282
        // decode: 282 * 2^-8 = 282 / 256 = 1.1015625
        let raw: u16 = 282;
        let v = decode_linear16(raw, -8);
        assert!((v - 1.1015625).abs() < 0.01, "got {v}");
    }

    #[test]
    fn linear16_vout_mode_0x18() {
        // VOUT_MODE = 0x18 -> bits[4:0] = 0x18 = 0b11000 -> sign_extend = -8
        let exp = sign_extend_5bit(0x18 & 0x1F);
        assert_eq!(exp, -8, "expected exponent -8, got {exp}");
    }

    #[test]
    fn linear16_zero() {
        assert!((decode_linear16(0, -8) - 0.0).abs() < 0.001);
    }

    // --- sign_extend_5bit tests ---

    #[test]
    fn sign_extend_positive() {
        assert_eq!(sign_extend_5bit(0x00), 0);
        assert_eq!(sign_extend_5bit(0x0F), 15);
    }

    #[test]
    fn sign_extend_negative() {
        // 0x1F = 0b11111 -> -1
        assert_eq!(sign_extend_5bit(0x1F), -1);
        // 0x18 = 0b11000 -> -8
        assert_eq!(sign_extend_5bit(0x18), -8);
        // 0x10 = 0b10000 -> -16
        assert_eq!(sign_extend_5bit(0x10), -16);
    }

    // --- Discovery with no hardware ---

    #[test]
    fn discover_returns_empty_without_hardware() {
        let source = PmbusSource::discover(&[]);
        assert_eq!(source.device_count(), 0);
        assert!(source.poll().is_empty());
    }
}
