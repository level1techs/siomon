//! DDR5 DIMM temperature sensors via direct I2C probing.
//!
//! On whitelisted boards, dedicated I2C controllers bypass the FCH's SPD mux
//! and expose three temperature sensors per DIMM:
//!
//! - **Hub** (0x50–0x53): SPD5118 hub die temperature
//! - **TS0** (0x30–0x33): DRAM die / sub-channel A temperature (TS5111)
//! - **TS1** (0x10–0x13): DRAM die / sub-channel B temperature (TS5111)
//!
//! Boards opt in by setting `ddr5_bus_config` in their `BoardTemplate`
//! (see `db/boards/mod.rs`). Requires `--direct-io`.

use crate::db::boards::{Ddr5BusConfig, Requirement};
use crate::model::sensor::{SensorCategory, SensorId, SensorReading, SensorUnit};
use crate::sensors::i2c::bus_scan;
use crate::sensors::i2c::ddr5;
use crate::sensors::i2c::smbus_io::SmbusDevice;

/// MR0 — device type register. 0x51 for both SPD5118 and TS5111.
const MR0_DEVICE_TYPE: u8 = 0x00;

/// MR11 — NVM page select register (bits [2:0] = page 0-7).
const MR11_PAGE_SELECT: u8 = 0x0B;

/// MR31 — temperature data register (16-bit word read).
const MR_TEMPERATURE: u8 = 0x31;

/// Expected device type for SPD5118/TS5111.
const JEDEC_DDR5_DEVICE_ID: u8 = 0x51;

/// Resolution of fractional temperature bits (°C per LSB).
const TEMP_LSB: f64 = 0.0625;

/// Build the sensor name component for a DDR5 temp sensor ID.
/// Shared between sensor discovery and TUI DIMM view matching.
pub fn sensor_name(bus: u32, hub_addr: u16, suffix: &str) -> String {
    format!("bus{}_{:#04x}_{}_temp", bus, hub_addr, suffix)
}

/// DDR5 I2C address ranges for each sensor type.
const HUB_ADDR_BASE: u16 = 0x50; // SPD5118 hub
const TS0_ADDR_BASE: u16 = 0x30; // TS5111 sub-channel A
const TS1_ADDR_BASE: u16 = 0x10; // TS5111 sub-channel B
const HUB_ADDR_COUNT: u16 = 4;
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
    dev: SmbusDevice,
    label: String,
    id: SensorId,
    is_hub: bool,
}

pub struct Ddr5TempSource {
    sensors: Vec<TempSensor>,
}

impl Ddr5TempSource {
    /// Discover DDR5 temperature sensors on whitelisted I2C buses.
    ///
    /// Returns an empty source if no DDR5 bus config or no devices are found.
    pub fn discover(
        ddr5_bus_config: Option<&Ddr5BusConfig>,
        ddr5_requirements: &[Requirement],
    ) -> Self {
        let Some(config) = ddr5_bus_config else {
            return Self {
                sensors: Vec::new(),
            };
        };

        let all_buses = bus_scan::enumerate_buses();
        let candidate_bus_nums = ddr5::filter_buses(config, &all_buses);

        if candidate_bus_nums.is_empty() {
            log::debug!("DDR5 temp: no whitelisted I2C buses found");
            return Self {
                sensors: Vec::new(),
            };
        }

        let mut sensors = Vec::new();
        let mut dimm_counter: u32 = 0;

        for &bus_num in &candidate_bus_nums {
            log::debug!("DDR5 temp: scanning bus {}", bus_num);
            for slot in 0..config.slots_per_bus {
                // Probe hub first to confirm a DIMM exists in this slot.
                let hub_addr = HUB_ADDR_BASE + slot;
                if !probe_ddr5_sensor(bus_num, hub_addr) {
                    log::debug!(
                        "DDR5 temp: no hub sensor on bus {} slot {} addr {:#04x}",
                        bus_num,
                        slot,
                        hub_addr
                    );
                    continue;
                }

                for sensor_type in [SensorType::Hub, SensorType::Ts0, SensorType::Ts1] {
                    let addr = sensor_type.base_addr() + slot;
                    if probe_ddr5_sensor(bus_num, addr) {
                        // Open a persistent handle for polling.
                        let dev = match SmbusDevice::open(bus_num, addr) {
                            Ok(dev) => dev,
                            Err(_) => continue,
                        };
                        let label = format!(
                            "DIMM {} {} (bus {} slot {})",
                            dimm_counter,
                            sensor_type.label_prefix(),
                            bus_num,
                            slot
                        );
                        // Sensor ID uses bus/hub_addr as a stable key that matches
                        // the SPD EEPROM's i2c_bus/i2c_addr on DimmInfo, rather
                        // than a sequential index that depends on enumeration order.
                        let id = SensorId {
                            source: "i2c".into(),
                            chip: sensor_type.chip_name().into(),
                            sensor: sensor_name(bus_num, hub_addr, sensor_type.sensor_suffix()),
                        };
                        log::debug!(
                            "DDR5 temp: found {} at bus {} addr {:#04x} -> {}",
                            sensor_type.label_prefix(),
                            bus_num,
                            addr,
                            id
                        );
                        let is_hub = matches!(sensor_type, SensorType::Hub);
                        sensors.push(TempSensor {
                            dev,
                            label,
                            id,
                            is_hub,
                        });
                    }
                }

                dimm_counter += 1;
            }
        }

        if sensors.is_empty() {
            let bios = crate::db::boards::diagnostics::read_bios_version();
            let hints = crate::db::boards::diagnostics::probe_failure_hints(
                "DDR5 temp",
                ddr5_requirements,
                bios.as_deref(),
            );
            for hint in &hints {
                log::warn!("DDR5 temp: {}", hint);
            }
        } else {
            log::info!("DDR5 temp: discovered {} sensor(s)", sensors.len());
        }

        Self { sensors }
    }

    fn poll(&self) -> Vec<(SensorId, SensorReading)> {
        let mut readings = Vec::new();

        for s in &self.sensors {
            // Hub sensors (SPD5118) can be left on a non-zero page by the SPD
            // EEPROM reader or by BMC/IPMI contention. Ensure page 0 before
            // reading temperature so MR31 maps to the volatile register set.
            if s.is_hub {
                let _ = s.dev.write_byte_data(MR11_PAGE_SELECT, 0x00);
            }
            match read_temperature_cached(&s.dev) {
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
                    log::warn!("DDR5 temp: read failed {}: {}", s.label, e);
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
    let dev = match SmbusDevice::open(bus, addr) {
        Ok(dev) => dev,
        Err(e) => {
            log::debug!(
                "DDR5 temp: open failed on bus {} addr {:#04x}: {}",
                bus,
                addr,
                e
            );
            return false;
        }
    };

    // Verify device type BEFORE any writes — writing to register 0x0B
    // on a non-SPD5118/TS5111 device could corrupt EEPROM data.
    let mr0 = match dev.read_byte_data(MR0_DEVICE_TYPE) {
        Ok(mr0) => mr0,
        Err(e) => {
            log::debug!(
                "DDR5 temp: MR0 read failed on bus {} addr {:#04x}: {}",
                bus,
                addr,
                e
            );
            return false;
        }
    };
    if mr0 != JEDEC_DDR5_DEVICE_ID {
        log::debug!(
            "DDR5 temp: MR0 mismatch on bus {} addr {:#04x}: got {:#04x}",
            bus,
            addr,
            mr0
        );
        return false;
    }

    // Confirmed SPD5118/TS5111 — reset page 0 on hub addresses so
    // volatile registers (including temperature) are accessible.
    if (HUB_ADDR_BASE..HUB_ADDR_BASE + HUB_ADDR_COUNT).contains(&addr) {
        let _ = dev.write_byte_data(MR11_PAGE_SELECT, 0x00);
    }

    // Verify plausible temperature.
    match dev.read_word_data(MR_TEMPERATURE) {
        Ok(raw) => {
            let masked = raw & 0x1FFF;
            // Sign-extend the 13-bit value (same as read_temperature_cached).
            let temp = if raw & 0x1000 != 0 {
                let signed = (masked as i16) | !0x1FFF_u16 as i16;
                (signed as f64) * TEMP_LSB
            } else {
                (masked as f64) * TEMP_LSB
            };
            if !(-40.0..=150.0).contains(&temp) {
                log::debug!(
                    "DDR5 temp: implausible temperature on bus {} addr {:#04x}: raw={:#06x} temp={:.2}",
                    bus,
                    addr,
                    raw,
                    temp
                );
                return false;
            }
        }
        Err(e) => {
            log::debug!(
                "DDR5 temp: temperature read failed on bus {} addr {:#04x}: {}",
                bus,
                addr,
                e
            );
            return false;
        }
    }

    true
}

/// Read temperature from MR31 using a cached device handle.
///
/// Encoding: 13-bit signed value in bits [12:0], 0.0625°C per LSB.
fn read_temperature_cached(dev: &SmbusDevice) -> std::io::Result<f64> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_none_returns_empty() {
        let source = Ddr5TempSource::discover(None, &[]);
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
        assert_eq!(sensor_name(1, 0x50, "hub"), "bus1_0x50_hub_temp");
        assert_eq!(sensor_name(2, 0x53, "ts1"), "bus2_0x53_ts1_temp");

        let id = SensorId {
            source: "i2c".into(),
            chip: SensorType::Ts0.chip_name().into(),
            sensor: sensor_name(1, 0x50, SensorType::Ts0.sensor_suffix()),
        };
        assert_eq!(id.to_string(), "i2c/ts5111/bus1_0x50_ts0_temp");
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
