//! DDR5 DIMM temperature sensors via DesignWare I2C buses.
//!
//! On AMD WRX90E, the DesignWare I2C controllers bypass the FCH's SPD mux
//! and expose three temperature sensors per DIMM:
//!
//! - **Hub** (0x50–0x53): SPD5118 hub die temperature
//! - **TS0** (0x30–0x33): DRAM die / sub-channel A temperature (TS5111)
//! - **TS1** (0x10–0x13): DRAM die / sub-channel B temperature (TS5111)
//!
//! Currently scoped to the ASUS WRX90E-SAGE SE board.

use crate::model::sensor::{SensorCategory, SensorId, SensorReading, SensorUnit};
use crate::platform::sysfs;
use crate::sensors::i2c::bus_scan::{self, I2cAdapterType, I2cBus};
use crate::sensors::i2c::smbus_io::SmbusDevice;

use std::path::Path;

/// MR0 — device type register. 0x51 for both SPD5118 and TS5111.
const MR0_DEVICE_TYPE: u8 = 0x00;

/// MR31 — temperature data register (16-bit word read).
const MR_TEMPERATURE: u8 = 0x31;

/// Expected device type for SPD5118/TS5111.
const JEDEC_DDR5_DEVICE_ID: u8 = 0x51;

/// Resolution of fractional temperature bits (°C per LSB).
const TEMP_LSB: f64 = 0.0625;

/// WRX90E-specific DesignWare I2C bus numbers.
const WRX90E_SPD_BUSES: &[u32] = &[1, 2];

/// DDR5 I2C address ranges for each sensor type.
const HUB_ADDR_BASE: u16 = 0x50; // SPD5118 hub
const TS0_ADDR_BASE: u16 = 0x30; // TS5111 sub-channel A
const TS1_ADDR_BASE: u16 = 0x10; // TS5111 sub-channel B
const SLOTS_PER_BUS: u16 = 4;

/// Type of DDR5 temperature sensor.
#[derive(Debug, Clone, Copy)]
enum SensorType {
    Hub,
    Ts0,
    Ts1,
}

impl SensorType {
    fn base_addr(self) -> u16 {
        match self {
            Self::Hub => HUB_ADDR_BASE,
            Self::Ts0 => TS0_ADDR_BASE,
            Self::Ts1 => TS1_ADDR_BASE,
        }
    }

    fn chip_name(self) -> &'static str {
        match self {
            Self::Hub => "spd5118",
            Self::Ts0 | Self::Ts1 => "ts5111",
        }
    }

    fn sensor_suffix(self) -> &'static str {
        match self {
            Self::Hub => "hub",
            Self::Ts0 => "ts0",
            Self::Ts1 => "ts1",
        }
    }

    fn label_prefix(self) -> &'static str {
        match self {
            Self::Hub => "Hub",
            Self::Ts0 => "TS0",
            Self::Ts1 => "TS1",
        }
    }
}

struct TempSensor {
    bus: u32,
    addr: u16,
    label: String,
    id: SensorId,
}

pub struct Ddr5TempSource {
    sensors: Vec<TempSensor>,
}

impl Ddr5TempSource {
    /// Discover DDR5 temperature sensors on WRX90E DesignWare I2C buses.
    ///
    /// Returns an empty source on non-WRX90E boards or if no devices are found.
    pub fn discover() -> Self {
        if !is_wrx90e() {
            return Self {
                sensors: Vec::new(),
            };
        }

        let all_buses = bus_scan::enumerate_buses();
        let dw_buses: Vec<&I2cBus> = all_buses
            .iter()
            .filter(|b| b.adapter_type == I2cAdapterType::DesignWare)
            .filter(|b| WRX90E_SPD_BUSES.contains(&b.bus_num))
            .collect();

        if dw_buses.is_empty() {
            log::debug!("DDR5 temp: no WRX90E DesignWare buses found");
            return Self {
                sensors: Vec::new(),
            };
        }

        let mut sensors = Vec::new();
        let mut dimm_index: u32 = 0;

        for bus in &dw_buses {
            for slot in 0..SLOTS_PER_BUS {
                // Probe hub first to confirm a DIMM exists in this slot.
                let hub_addr = HUB_ADDR_BASE + slot;
                if !probe_ddr5_sensor(bus.bus_num, hub_addr) {
                    continue;
                }

                for sensor_type in [SensorType::Hub, SensorType::Ts0, SensorType::Ts1] {
                    let addr = sensor_type.base_addr() + slot;
                    if probe_ddr5_sensor(bus.bus_num, addr) {
                        let label = format!(
                            "DIMM {} {} (bus {} slot {})",
                            dimm_index,
                            sensor_type.label_prefix(),
                            bus.bus_num,
                            slot
                        );
                        let id = SensorId {
                            source: "i2c".into(),
                            chip: sensor_type.chip_name().into(),
                            sensor: format!(
                                "dimm{}_{}_temp",
                                dimm_index,
                                sensor_type.sensor_suffix()
                            ),
                        };
                        log::info!(
                            "DDR5 temp: found {} at bus {} addr {:#04x} -> {}",
                            sensor_type.label_prefix(),
                            bus.bus_num,
                            addr,
                            id
                        );
                        sensors.push(TempSensor {
                            bus: bus.bus_num,
                            addr,
                            label,
                            id,
                        });
                    }
                }

                dimm_index += 1;
            }
        }

        if sensors.is_empty() {
            log::debug!("DDR5 temp: no sensors discovered");
        } else {
            log::info!("DDR5 temp: discovered {} sensor(s)", sensors.len());
        }

        Self { sensors }
    }

    fn poll(&self) -> Vec<(SensorId, SensorReading)> {
        let mut readings = Vec::new();

        for s in &self.sensors {
            match read_temperature(s.bus, s.addr) {
                Ok(temp_c) => {
                    readings.push((
                        s.id.clone(),
                        SensorReading::new(
                            s.label.clone(),
                            temp_c,
                            SensorUnit::Celsius,
                            SensorCategory::Temperature,
                        ),
                    ));
                }
                Err(e) => {
                    log::warn!(
                        "DDR5 temp: read failed {} (bus {} addr {:#04x}): {}",
                        s.label,
                        s.bus,
                        s.addr,
                        e
                    );
                }
            }
        }

        readings
    }

    #[cfg(test)]
    fn sensor_count(&self) -> usize {
        self.sensors.len()
    }
}

impl crate::sensors::SensorSource for Ddr5TempSource {
    fn name(&self) -> &str {
        "i2c"
    }

    fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
        Ddr5TempSource::poll(self)
    }
}

/// Probe a DDR5 device: verify MR0 = 0x51 and temperature is plausible.
///
/// Resets MR11 to page 0 first, since a prior aborted SPD EEPROM read may
/// have left the device with volatile registers disabled.
fn probe_ddr5_sensor(bus: u32, addr: u16) -> bool {
    let Ok(dev) = SmbusDevice::open(bus, addr) else {
        return false;
    };

    // Ensure page 0 is active so volatile registers are accessible.
    if (HUB_ADDR_BASE..=HUB_ADDR_BASE + SLOTS_PER_BUS).contains(&addr) {
        let _ = dev.write_byte_data(0x0B, 0x00);
    }

    let Ok(mr0) = dev.read_byte_data(MR0_DEVICE_TYPE) else {
        return false;
    };
    if mr0 != JEDEC_DDR5_DEVICE_ID {
        return false;
    }

    // Verify plausible temperature.
    if let Ok(raw) = dev.read_word_data(MR_TEMPERATURE) {
        let masked = raw & 0x1FFF;
        let temp = masked as f64 * TEMP_LSB;
        if !(-40.0..=150.0).contains(&temp) {
            return false;
        }
    }

    true
}

/// Read temperature from MR31 and convert to degrees Celsius.
///
/// Encoding: 13-bit signed value in bits [12:0], 0.0625°C per LSB.
fn read_temperature(bus: u32, addr: u16) -> std::io::Result<f64> {
    let dev = SmbusDevice::open(bus, addr)?;
    let raw = dev.read_word_data(MR_TEMPERATURE)?;

    let masked = raw & 0x1FFF;
    let temp_c = if raw & 0x1000 != 0 {
        let signed = (masked as i16) | !0x1FFF_u16 as i16;
        (signed as f64) * TEMP_LSB
    } else {
        (masked as f64) * TEMP_LSB
    };

    Ok(temp_c)
}

fn is_wrx90e() -> bool {
    sysfs::read_string_optional(Path::new("/sys/class/dmi/id/board_name"))
        .map(|n| n.to_lowercase().contains("wrx90e"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_without_hardware() {
        // On non-WRX90E or without hardware, discover returns empty.
        // This test verifies the struct works even when empty.
        let source = Ddr5TempSource {
            sensors: Vec::new(),
        };
        assert_eq!(source.sensor_count(), 0);
        assert!(source.poll().is_empty());
    }

    #[test]
    fn sensor_type_properties() {
        assert_eq!(SensorType::Hub.base_addr(), 0x50);
        assert_eq!(SensorType::Ts0.base_addr(), 0x30);
        assert_eq!(SensorType::Ts1.base_addr(), 0x10);

        assert_eq!(SensorType::Hub.chip_name(), "spd5118");
        assert_eq!(SensorType::Ts0.chip_name(), "ts5111");
        assert_eq!(SensorType::Ts1.chip_name(), "ts5111");
    }

    #[test]
    fn sensor_id_format() {
        let id = SensorId {
            source: "i2c".into(),
            chip: SensorType::Ts0.chip_name().into(),
            sensor: format!("dimm0_{}_temp", SensorType::Ts0.sensor_suffix()),
        };
        assert_eq!(id.to_string(), "i2c/ts5111/dimm0_ts0_temp");
    }

    /// Verify temperature decoding matches SPD5118's encoding.
    #[test]
    fn temp_decoding_positive() {
        // 0x02E4 = 740 * 0.0625 = 46.25°C
        let raw: u16 = 0x02E4;
        let masked = raw & 0x1FFF;
        let temp = masked as f64 * TEMP_LSB;
        assert!((temp - 46.25).abs() < 0.001);
    }

    #[test]
    fn temp_decoding_negative() {
        // -25.0°C: 2's complement of 400 in 13-bit = 0x1E70
        let raw: u16 = 0x1E70;
        let masked = raw & 0x1FFF;
        let signed = (masked as i16) | !0x1FFF_u16 as i16;
        let temp = (signed as f64) * TEMP_LSB;
        assert!((temp - (-25.0)).abs() < 0.001, "got {temp}");
    }
}
