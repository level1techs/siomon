use crate::model::sensor::{SensorCategory, SensorId, SensorReading, SensorUnit};

use super::bus_scan::I2cBus;
use super::smbus_io::SmbusDevice;

/// SPD5118 register addresses (Management Registers via SMBus).
///
/// MR0: Device type — should read 0x51 for SPD5118.
const SPD5118_MR_DEVICE_TYPE: u8 = 0x00;

/// MR31: Temperature sensor data (TS_DATA), 16-bit signed value.
///
/// Bits [12:4] are the integer part (9 bits, signed in bit 12).
/// Bits [3:0] are the fractional part at 0.0625 C per LSB.
const SPD5118_MR_TEMPERATURE: u8 = 0x31;

/// Expected device-type byte identifying an SPD5118 hub.
const SPD5118_DEVICE_TYPE_ID: u8 = 0x51;

/// The 7-bit I2C address range for SPD EEPROM/hub devices on DDR5.
const SPD_ADDR_FIRST: u16 = 0x50;
const SPD_ADDR_LAST: u16 = 0x57;

/// Resolution of the fractional temperature bits (degrees C per LSB).
const TEMP_LSB_RESOLUTION: f64 = 0.0625;

/// Sensor source for DDR5 DIMM temperatures read via SPD5118 on-die sensor.
pub struct Spd5118Source {
    dimms: Vec<DimmSensor>,
}

struct DimmSensor {
    bus: u32,
    addr: u16,
    label: String,
    id: SensorId,
}

impl Spd5118Source {
    /// Scan all SMBus adapters for SPD5118 devices and build the sensor list.
    ///
    /// Returns an empty source (with no DIMM entries) if no devices are found
    /// or if `/dev/i2c-*` cannot be opened (e.g., insufficient permissions).
    pub fn discover(buses: &[I2cBus]) -> Self {
        let mut dimms = Vec::new();
        let mut dimm_index: u32 = 0;

        for bus in buses {
            if !bus.adapter_type.is_smbus() {
                continue;
            }

            for addr in SPD_ADDR_FIRST..=SPD_ADDR_LAST {
                if let Some(dimm) = probe_spd5118(bus.bus_num, addr, dimm_index) {
                    log::info!(
                        "SPD5118 DIMM found: bus {} addr {:#04x} -> {}",
                        bus.bus_num,
                        addr,
                        dimm.label
                    );
                    dimm_index += 1;
                    dimms.push(dimm);
                }
            }
        }

        if dimms.is_empty() {
            log::debug!("No SPD5118 DIMM temperature sensors discovered");
        } else {
            log::info!(
                "Discovered {} SPD5118 DIMM temperature sensor(s)",
                dimms.len()
            );
        }

        Self { dimms }
    }

    /// Read current temperature from each discovered DIMM.
    pub fn poll(&self) -> Vec<(SensorId, SensorReading)> {
        let mut readings = Vec::new();

        for dimm in &self.dimms {
            match read_temperature(dimm.bus, dimm.addr) {
                Ok(temp_c) => {
                    let reading = SensorReading::new(
                        dimm.label.clone(),
                        temp_c,
                        SensorUnit::Celsius,
                        SensorCategory::Temperature,
                    );
                    readings.push((dimm.id.clone(), reading));
                }
                Err(e) => {
                    log::warn!(
                        "Failed to read temperature from {} (bus {} addr {:#04x}): {}",
                        dimm.label,
                        dimm.bus,
                        dimm.addr,
                        e
                    );
                }
            }
        }

        readings
    }

    /// Number of discovered DIMM sensors.
    pub fn dimm_count(&self) -> usize {
        self.dimms.len()
    }
}

/// Attempt to open the device and verify it is an SPD5118 by reading MR0.
///
/// Note: On AMD platforms (piix4_smbus), the FCH's SPD controller may
/// intercept reads at 0x50-0x57 and return EEPROM data instead of
/// management registers. In that case, MR0 will not read 0x51 and
/// the probe will correctly reject the device.
fn probe_spd5118(bus: u32, addr: u16, dimm_index: u32) -> Option<DimmSensor> {
    let dev = SmbusDevice::open(bus, addr).ok()?;

    // Read device type register — must be 0x51 for SPD5118.
    // On AMD FCH platforms, this may return EEPROM data instead of MR0.
    let device_type = dev.read_byte_data(SPD5118_MR_DEVICE_TYPE).ok()?;
    if device_type != SPD5118_DEVICE_TYPE_ID {
        log::debug!(
            "SPD5118 probe: bus {} addr {:#04x} MR0={:#04x} (expected {:#04x})",
            bus,
            addr,
            device_type,
            SPD5118_DEVICE_TYPE_ID
        );
        return None;
    }

    // Verify the temperature register returns a plausible value
    let temp_raw = dev.read_word_data(SPD5118_MR_TEMPERATURE).ok()?;
    let masked = temp_raw & 0x1FFF;
    let temp_c = masked as f64 * TEMP_LSB_RESOLUTION;
    if !(-40.0..=150.0).contains(&temp_c) {
        log::debug!(
            "SPD5118 probe: bus {} addr {:#04x} temp {:.1}C out of range",
            bus,
            addr,
            temp_c
        );
        return None;
    }

    let slot = addr - SPD_ADDR_FIRST;
    let label = format!("DIMM {} (bus {} slot {})", dimm_index, bus, slot);
    let id = SensorId {
        source: "i2c".into(),
        chip: "spd5118".into(),
        sensor: format!("dimm{dimm_index}_temp"),
    };

    Some(DimmSensor {
        bus,
        addr,
        label,
        id,
    })
}

/// Read the SPD5118 temperature register and convert to degrees Celsius.
///
/// The MR31 register returns a 16-bit value with the following layout:
///
/// ```text
///   Bit 15..13: reserved
///   Bit 12:     sign (1 = negative)
///   Bit 11..4:  integer magnitude (8 bits for positive, 2's complement with sign for negative)
///   Bit 3..0:   fractional part (0.0625 C per LSB)
/// ```
///
/// For positive values: temperature = bits[11:4] + bits[3:0] * 0.0625
/// For negative values: the 13-bit field [12:0] is a signed 2's complement
/// value scaled by 16 (i.e., divide by 16.0 to get degrees).
fn read_temperature(bus: u32, addr: u16) -> std::io::Result<f64> {
    let dev = SmbusDevice::open(bus, addr)?;
    let raw = dev.read_word_data(SPD5118_MR_TEMPERATURE)?;

    // Mask to 13 significant bits [12:0]
    let masked = raw & 0x1FFF;

    let temp_c = if raw & 0x1000 != 0 {
        // Negative temperature: sign-extend the 13-bit value to i16
        let signed = (masked as i16) | !0x1FFF_u16 as i16;
        (signed as f64) * TEMP_LSB_RESOLUTION
    } else {
        (masked as f64) * TEMP_LSB_RESOLUTION
    };

    Ok(temp_c)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to convert a raw 16-bit register value to temperature,
    /// exercising the same logic as `read_temperature` without I2C access.
    fn raw_to_temp(raw: u16) -> f64 {
        let masked = raw & 0x1FFF;
        if raw & 0x1000 != 0 {
            let signed = (masked as i16) | !0x1FFF_u16 as i16;
            (signed as f64) * TEMP_LSB_RESOLUTION
        } else {
            (masked as f64) * TEMP_LSB_RESOLUTION
        }
    }

    #[test]
    fn temp_zero() {
        assert!((raw_to_temp(0x0000) - 0.0).abs() < 0.001);
    }

    #[test]
    fn temp_25_degrees() {
        // 25.0 C = 400 * 0.0625 = 25.0; 400 = 0x0190
        assert!((raw_to_temp(0x0190) - 25.0).abs() < 0.001);
    }

    #[test]
    fn temp_25_0625() {
        // 25.0625 C = 401 * 0.0625; 401 = 0x0191
        assert!((raw_to_temp(0x0191) - 25.0625).abs() < 0.001);
    }

    #[test]
    fn temp_85_degrees() {
        // 85.0 C = 1360 * 0.0625; 1360 = 0x0550
        assert!((raw_to_temp(0x0550) - 85.0).abs() < 0.001);
    }

    #[test]
    fn temp_negative_25() {
        // -25.0 C: 2's complement of 400 in 13-bit = 0x1FFF - 400 + 1 = 0x1E70
        // Actually: -25.0 / 0.0625 = -400; as 13-bit 2's comp: 8192 - 400 = 7792 = 0x1E70
        let raw = 0x1E70_u16;
        let t = raw_to_temp(raw);
        assert!((t - (-25.0)).abs() < 0.001, "got {t}");
    }

    #[test]
    fn temp_negative_0_0625() {
        // -0.0625 C: 2's complement of 1 in 13-bit = 0x1FFF
        let t = raw_to_temp(0x1FFF);
        assert!((t - (-0.0625)).abs() < 0.001, "got {t}");
    }

    #[test]
    fn temp_max_positive() {
        // Maximum positive: 0x0FFF = 4095 * 0.0625 = 255.9375 C
        let t = raw_to_temp(0x0FFF);
        assert!((t - 255.9375).abs() < 0.001, "got {t}");
    }

    #[test]
    fn discover_returns_empty_without_hardware() {
        // With no buses, discover should return an empty source
        let source = Spd5118Source::discover(&[]);
        assert_eq!(source.dimm_count(), 0);
        assert!(source.poll().is_empty());
    }
}
