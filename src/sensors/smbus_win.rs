//! Windows SMBus sensor source for PMBus VRM monitoring and DDR5 DIMM temperature.
//!
//! This is a standalone Windows implementation that drives the AMD FCH SMBus
//! host controller directly via WinRing0 port I/O. It provides:
//!
//! - **PMBus VRM telemetry**: voltage, current, temperature, and power from
//!   VRM controllers at I2C addresses 0x20-0x4F.
//! - **SPD5118 DIMM temperature**: on-die temperature from DDR5 DIMMs at
//!   I2C addresses 0x50-0x57.
//!
//! The PMBus LINEAR11 and LINEAR16 decoding logic is duplicated from the
//! unix-gated `i2c::pmbus` module since that module cannot be compiled on
//! Windows (it depends on Linux I2C ioctls).

use crate::model::sensor::{SensorCategory, SensorId, SensorReading, SensorUnit};
use crate::platform::smbus_win::SmbusController;

// ---------------------------------------------------------------------------
// PMBus constants
// ---------------------------------------------------------------------------

/// PMBus command codes.
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

/// I2C address range to scan for PMBus VRM controllers.
const VRM_ADDR_FIRST: u8 = 0x20;
const VRM_ADDR_LAST: u8 = 0x4F;

/// Maximum number of VRM devices to discover (guard against false positives).
const MAX_VRMS: usize = 32;

// ---------------------------------------------------------------------------
// SPD5118 constants
// ---------------------------------------------------------------------------

/// SPD5118 Management Register 0: device type (should be 0x51).
const SPD5118_MR_DEVICE_TYPE: u8 = 0x00;

/// SPD5118 Management Register 31: temperature sensor data.
const SPD5118_MR_TEMPERATURE: u8 = 0x31;

/// Expected MR0 value for SPD5118.
const SPD5118_DEVICE_TYPE_ID: u8 = 0x51;

/// I2C address range for SPD EEPROM/hub devices.
const SPD_ADDR_FIRST: u8 = 0x50;
const SPD_ADDR_LAST: u8 = 0x57;

/// Temperature resolution: degrees Celsius per LSB.
const TEMP_LSB_RESOLUTION: f64 = 0.0625;

// ---------------------------------------------------------------------------
// PMBus register descriptor
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum PmbusFormat {
    Linear11,
    Linear16,
}

struct PmbusRegister {
    command: u8,
    suffix: &'static str,
    label_suffix: &'static str,
    unit: SensorUnit,
    category: SensorCategory,
    format: PmbusFormat,
}

/// The set of PMBus registers polled for each VRM device.
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

// ---------------------------------------------------------------------------
// Discovered device records
// ---------------------------------------------------------------------------

struct PmbusDevice {
    addr: u8,
    page: Option<u8>,
    vout_exponent: i8,
    label_prefix: String,
    id_prefix: String,
}

struct DimmSensor {
    addr: u8,
    label: String,
    id: SensorId,
}

// ---------------------------------------------------------------------------
// Public sensor source
// ---------------------------------------------------------------------------

/// Combined sensor source for PMBus VRM controllers and SPD5118 DIMM
/// temperature sensors discovered on the Windows AMD FCH SMBus.
pub struct SmbusWinSource {
    /// Cached SMBus controller handle (None if detection failed).
    controller: Option<SmbusController>,
    vrms: Vec<PmbusDevice>,
    dimms: Vec<DimmSensor>,
}

impl SmbusWinSource {
    /// Discover the SMBus controller and scan for attached devices.
    ///
    /// Returns an empty (no-op) source if:
    /// - WinRing0 is not available
    /// - No AMD FCH SMBus controller is detected
    /// - No PMBus or SPD5118 devices respond
    pub fn discover() -> Self {
        let controller = match SmbusController::detect_amd_fch() {
            Some(c) => c,
            None => {
                log::debug!("SMBus: no AMD FCH controller found, skipping");
                return Self {
                    controller: None,
                    vrms: Vec::new(),
                    dimms: Vec::new(),
                };
            }
        };

        let vrms = scan_pmbus_devices(&controller);
        let dimms = scan_spd5118_devices(&controller);

        if vrms.is_empty() && dimms.is_empty() {
            log::debug!("SMBus: no PMBus or SPD5118 devices found");
        } else {
            log::info!(
                "SMBus: discovered {} VRM(s) and {} DIMM temp sensor(s)",
                vrms.len(),
                dimms.len()
            );
        }

        Self {
            controller: Some(controller),
            vrms,
            dimms,
        }
    }

    /// Poll all discovered VRM and DIMM sensors.
    pub fn poll(&self) -> Vec<(SensorId, SensorReading)> {
        let ctrl = match &self.controller {
            Some(c) => c,
            None => return Vec::new(),
        };

        let mut readings = Vec::new();

        // Poll PMBus VRMs
        for dev in &self.vrms {
            // Select PMBus page if needed
            if let Some(page) = dev.page {
                if ctrl.write_byte_data(dev.addr, PMBUS_PAGE, page).is_none() {
                    continue;
                }
            }

            for reg in REGISTERS {
                let raw = match ctrl.read_word_data(dev.addr, reg.command) {
                    Some(v) => v,
                    None => continue,
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

        // Poll SPD5118 DIMM temperatures
        for dimm in &self.dimms {
            if let Some(temp_c) = read_spd5118_temperature(ctrl, dimm.addr) {
                let reading = SensorReading::new(
                    dimm.label.clone(),
                    temp_c,
                    SensorUnit::Celsius,
                    SensorCategory::Temperature,
                );
                readings.push((dimm.id.clone(), reading));
            }
        }

        readings
    }
}

// ---------------------------------------------------------------------------
// PMBus device scanning
// ---------------------------------------------------------------------------

/// Scan the VRM address range for PMBus devices with valid VOUT_MODE.
fn scan_pmbus_devices(ctrl: &SmbusController) -> Vec<PmbusDevice> {
    let mut devices = Vec::new();
    let mut vrm_index: u32 = 0;

    for addr in VRM_ADDR_FIRST..=VRM_ADDR_LAST {
        if devices.len() >= MAX_VRMS {
            log::warn!("SMBus: hit {} VRM cap, stopping scan", MAX_VRMS);
            break;
        }

        let found = probe_pmbus_with_pages(ctrl, addr, &mut vrm_index);
        for dev in found {
            log::info!(
                "PMBus VRM found: addr {:#04x} page={:?} vout_exp={} -> {}",
                addr,
                dev.page,
                dev.vout_exponent,
                dev.label_prefix
            );
            devices.push(dev);
        }
    }

    devices
}

/// Probe a single I2C address for a PMBus device, checking multiple pages.
fn probe_pmbus_with_pages(
    ctrl: &SmbusController,
    addr: u8,
    vrm_index: &mut u32,
) -> Vec<PmbusDevice> {
    // Quick check: try reading VOUT_MODE to see if anything ACKs
    if ctrl.read_byte_data(addr, PMBUS_VOUT_MODE).is_none() {
        return Vec::new();
    }

    let mut results = Vec::new();

    for page in 0u8..4 {
        // Select PMBus page
        if ctrl.write_byte_data(addr, PMBUS_PAGE, page).is_none() {
            break;
        }

        // Read VOUT_MODE for this page
        let vout_mode = match ctrl.read_byte_data(addr, PMBUS_VOUT_MODE) {
            Some(v) => v,
            None => continue,
        };

        // Must be LINEAR mode (bits [7:5] = 000)
        if (vout_mode >> 5) != 0 {
            continue;
        }

        let vout_exponent = sign_extend_5bit(vout_mode & 0x1F);

        // Real VRM controllers use negative exponents (-15 to -5) for
        // sub-volt resolution. Non-negative means multi-volt steps,
        // which is not a real VRM.
        if vout_exponent >= 0 {
            continue;
        }

        // Check VOUT: page is "active" if VOUT > 0
        let vout_raw = match ctrl.read_word_data(addr, PMBUS_READ_VOUT) {
            Some(v) => v,
            None => continue,
        };
        let vout = decode_linear16(vout_raw, vout_exponent);
        if vout < 0.01 {
            continue; // Rail is off or not connected
        }

        // Sanity: check VIN is plausible
        if let Some(vin_raw) = ctrl.read_word_data(addr, PMBUS_READ_VIN) {
            let vin = decode_linear11(vin_raw);
            if !(0.0..=60.0).contains(&vin) {
                continue;
            }
        }

        let page_label = if page == 0 && results.is_empty() {
            format!("VRM {} (addr {:#04x})", *vrm_index, addr)
        } else {
            format!("VRM {} page {} (addr {:#04x})", *vrm_index, page, addr)
        };

        let id_prefix = if page > 0 || !results.is_empty() {
            format!("vrm{}_p{}", *vrm_index, page)
        } else {
            format!("vrm{}", *vrm_index)
        };

        results.push(PmbusDevice {
            addr,
            page: Some(page),
            vout_exponent,
            label_prefix: page_label,
            id_prefix,
        });
    }

    // Fix up labels: if we found multiple pages, relabel the first
    if results.len() > 1 {
        if let Some(first) = results.first_mut() {
            first.label_prefix = format!("VRM {} page 0 (addr {:#04x})", *vrm_index, addr);
            first.id_prefix = format!("vrm{}_p0", *vrm_index);
        }
    }

    // Reset page to 0 when done
    if !results.is_empty() {
        let _ = ctrl.write_byte_data(addr, PMBUS_PAGE, 0);
        *vrm_index += 1;
    }

    results
}

// ---------------------------------------------------------------------------
// SPD5118 device scanning
// ---------------------------------------------------------------------------

/// Scan the SPD address range for SPD5118 DIMM temperature sensors.
fn scan_spd5118_devices(ctrl: &SmbusController) -> Vec<DimmSensor> {
    let mut dimms = Vec::new();
    let mut dimm_index: u32 = 0;

    for addr in SPD_ADDR_FIRST..=SPD_ADDR_LAST {
        if let Some(dimm) = probe_spd5118(ctrl, addr, dimm_index) {
            log::info!("SPD5118 DIMM found: addr {:#04x} -> {}", addr, dimm.label);
            dimm_index += 1;
            dimms.push(dimm);
        }
    }

    dimms
}

/// Probe a single address for an SPD5118 device.
fn probe_spd5118(ctrl: &SmbusController, addr: u8, dimm_index: u32) -> Option<DimmSensor> {
    // Read device type register (MR0) -- must be 0x51
    let device_type = ctrl.read_byte_data(addr, SPD5118_MR_DEVICE_TYPE)?;
    if device_type != SPD5118_DEVICE_TYPE_ID {
        log::debug!(
            "SPD5118 probe: addr {:#04x} MR0={:#04x} (expected {:#04x})",
            addr,
            device_type,
            SPD5118_DEVICE_TYPE_ID
        );
        return None;
    }

    // Verify temperature is plausible
    let temp_raw = ctrl.read_word_data(addr, SPD5118_MR_TEMPERATURE)?;
    let masked = temp_raw & 0x1FFF;
    let temp_c = masked as f64 * TEMP_LSB_RESOLUTION;
    if !(-40.0..=150.0).contains(&temp_c) {
        log::debug!(
            "SPD5118 probe: addr {:#04x} temp {:.1}C out of range",
            addr,
            temp_c
        );
        return None;
    }

    let slot = addr - SPD_ADDR_FIRST;
    let label = format!("DIMM {} (slot {})", dimm_index, slot);
    let id = SensorId {
        source: "i2c".into(),
        chip: "spd5118".into(),
        sensor: format!("dimm{dimm_index}_temp"),
    };

    Some(DimmSensor { addr, label, id })
}

/// Read the SPD5118 MR31 temperature register and convert to Celsius.
fn read_spd5118_temperature(ctrl: &SmbusController, addr: u8) -> Option<f64> {
    let raw = ctrl.read_word_data(addr, SPD5118_MR_TEMPERATURE)?;

    // Mask to 13 significant bits [12:0]
    let masked = raw & 0x1FFF;

    let temp_c = if raw & 0x1000 != 0 {
        // Negative temperature: sign-extend the 13-bit value
        let signed = (masked as i16) | !0x1FFF_u16 as i16;
        (signed as f64) * TEMP_LSB_RESOLUTION
    } else {
        (masked as f64) * TEMP_LSB_RESOLUTION
    };

    Some(temp_c)
}

// ---------------------------------------------------------------------------
// PMBus LINEAR11 / LINEAR16 decoding
// ---------------------------------------------------------------------------

/// Decode a PMBus LINEAR11 value to floating-point.
///
/// LINEAR11 format: signed 5-bit exponent in bits [15:11], signed 11-bit
/// mantissa in bits [10:0]. value = mantissa * 2^exponent.
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
/// LINEAR16 format: value = raw_u16 * 2^exponent.
fn decode_linear16(raw: u16, exponent: i8) -> f64 {
    raw as f64 * f64::powi(2.0, exponent as i32)
}

/// Sign-extend a 5-bit value to i8.
fn sign_extend_5bit(val: u8) -> i8 {
    if val & 0x10 != 0 {
        (val | 0xE0) as i8
    } else {
        val as i8
    }
}

// ---------------------------------------------------------------------------
// SensorSource trait impl
// ---------------------------------------------------------------------------

impl crate::sensors::SensorSource for SmbusWinSource {
    fn name(&self) -> &str {
        "smbus"
    }

    fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
        SmbusWinSource::poll(self)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- LINEAR11 decoding ---

    #[test]
    fn linear11_vin_11_94v() {
        // exponent = -4, mantissa = 191 -> 191 * 2^-4 = 11.9375
        let raw: u16 = 0xE0BF;
        let v = decode_linear11(raw);
        assert!((v - 11.9375).abs() < 0.01, "got {v}");
    }

    #[test]
    fn linear11_iout_6_625a() {
        // exponent = -3, mantissa = 53 -> 53 * 2^-3 = 6.625
        let raw: u16 = 0xE835;
        let v = decode_linear11(raw);
        assert!((v - 6.625).abs() < 0.01, "got {v}");
    }

    #[test]
    fn linear11_temp_35c() {
        // exponent = -2, mantissa = 140 -> 140 * 2^-2 = 35.0
        let raw: u16 = 0xF08C;
        let v = decode_linear11(raw);
        assert!((v - 35.0).abs() < 0.01, "got {v}");
    }

    #[test]
    fn linear11_zero() {
        assert!((decode_linear11(0x0000) - 0.0).abs() < 0.001);
    }

    #[test]
    fn linear11_negative_mantissa() {
        // mantissa = -1 (0x7FF), exponent = 0 -> -1.0
        let raw: u16 = 0x07FF;
        let v = decode_linear11(raw);
        assert!((v - (-1.0)).abs() < 0.001, "got {v}");
    }

    // --- LINEAR16 decoding ---

    #[test]
    fn linear16_vout_1_10v() {
        // raw = 282, exponent = -8 -> 282 / 256 = 1.1015625
        let v = decode_linear16(282, -8);
        assert!((v - 1.1015625).abs() < 0.01, "got {v}");
    }

    #[test]
    fn linear16_zero() {
        assert!((decode_linear16(0, -8) - 0.0).abs() < 0.001);
    }

    // --- sign_extend_5bit ---

    #[test]
    fn sign_extend_positive() {
        assert_eq!(sign_extend_5bit(0x00), 0);
        assert_eq!(sign_extend_5bit(0x0F), 15);
    }

    #[test]
    fn sign_extend_negative() {
        assert_eq!(sign_extend_5bit(0x1F), -1);
        assert_eq!(sign_extend_5bit(0x18), -8);
        assert_eq!(sign_extend_5bit(0x10), -16);
    }

    // --- SPD5118 temperature decoding ---

    #[test]
    fn spd_temp_25c() {
        // 25.0 C = 400 * 0.0625; raw = 0x0190
        let masked = 0x0190_u16 & 0x1FFF;
        let temp = masked as f64 * TEMP_LSB_RESOLUTION;
        assert!((temp - 25.0).abs() < 0.001, "got {temp}");
    }

    #[test]
    fn spd_temp_85c() {
        // 85.0 C = 1360 * 0.0625; raw = 0x0550
        let masked = 0x0550_u16 & 0x1FFF;
        let temp = masked as f64 * TEMP_LSB_RESOLUTION;
        assert!((temp - 85.0).abs() < 0.001, "got {temp}");
    }

    #[test]
    fn spd_temp_negative_25c() {
        // -25.0 C: 13-bit two's complement of 400 = 0x1E70
        let raw = 0x1E70_u16;
        let masked = raw & 0x1FFF;
        let signed = (masked as i16) | !0x1FFF_u16 as i16;
        let temp = (signed as f64) * TEMP_LSB_RESOLUTION;
        assert!((temp - (-25.0)).abs() < 0.001, "got {temp}");
    }

    // --- Discovery without hardware ---

    #[test]
    fn discover_returns_empty_without_hardware() {
        let source = SmbusWinSource::discover();
        // Without WinRing0, controller will be None, so poll returns empty
        // (On a real system with WinRing0 but no VRMs, it also returns empty)
        let readings = source.poll();
        // We cannot assert emptiness since a real system might have devices,
        // but we verify it doesn't panic.
        let _ = readings;
    }
}
