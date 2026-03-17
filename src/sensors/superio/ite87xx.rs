//! Direct register reader for ITE IT8686E/IT8688E/IT8689E Super I/O chips.
//!
//! Common on Gigabyte motherboards. Reads voltages, temperatures, and fan
//! speeds directly from the chip's hardware monitoring registers via I/O ports.
//! Requires root or CAP_SYS_RAWIO.
//!
//! Register map derived from kernel drivers/hwmon/it87.c.

use crate::model::sensor::{SensorCategory, SensorId, SensorReading, SensorUnit};
#[cfg(unix)]
use crate::platform::port_io::PortIo;
#[cfg(windows)]
use crate::platform::port_io_win::PortIo;
use crate::sensors::superio::chip_detect::{ChipType, SuperIoChip};

// Voltage input registers (direct offset, no banking needed)
// From kernel: IT87_REG_VIN[] = {0x20..0x28, 0x2f, 0x2c, 0x2d, 0x2e}
const VOLTAGE_REGS: [u8; 13] = [
    0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x2F, 0x2C, 0x2D, 0x2E,
];

const VOLTAGE_LABELS: [&str; 13] = [
    "VIN0", "VIN1", "VIN2", "AVCC", "VIN4", "VIN5", "VIN6", "VSB", "Vbat", "VIN9", "VIN10",
    "VIN11", "VIN12",
];

// Channels with internal 2x scaling (voltage divider in the chip)
// IT8686/8688/8689: in3=AVCC, in7=VSB, in8=Vbat, in9=AVCC3
const SCALED_CHANNELS: [usize; 4] = [3, 7, 8, 9];

// Temperature registers (3 primary channels)
const TEMP_REGS: [u8; 3] = [0x29, 0x2A, 0x2B];
const TEMP_LABELS: [&str; 3] = ["System Temp", "CPU Temp", "Auxiliary Temp"];

// Fan tachometer registers (16-bit extended, 6 fans)
// FANX registers give full 16-bit count
const FANX_REGS: [(u8, u8); 6] = [
    (0x18, 0x0D), // Fan 1: FANX=0x18, FAN=0x0D
    (0x19, 0x0E), // Fan 2
    (0x1A, 0x0F), // Fan 3
    (0x81, 0x80), // Fan 4
    (0x83, 0x82), // Fan 5
    (0x4D, 0x4C), // Fan 6
];

const FAN_LABELS: [&str; 6] = ["Fan 1", "Fan 2", "Fan 3", "Fan 4", "Fan 5", "Fan 6"];

// RPM calculation constant (same as Nuvoton)
const FAN_RPM_FACTOR: u32 = 1_350_000;

// 16-bit fan enable register
const REG_FAN_16BIT: u8 = 0x0C;

pub struct Ite87xxSource {
    chip: SuperIoChip,
    addr_port: u16,
    data_port: u16,
    has_12mv_adc: bool,
}

impl Ite87xxSource {
    pub fn new(chip: SuperIoChip) -> Self {
        let addr_port = chip.hwm_base + 5;
        let data_port = chip.hwm_base + 6;
        // IT8686E and IT8688E use 12mV ADC; IT8689E uses 16mV
        let has_12mv_adc = matches!(chip.chip, ChipType::Ite8686 | ChipType::Ite8688);
        Self {
            chip,
            addr_port,
            data_port,
            has_12mv_adc,
        }
    }

    pub fn is_supported(&self) -> bool {
        matches!(
            self.chip.chip,
            ChipType::Ite8686 | ChipType::Ite8688 | ChipType::Ite8689
        )
    }

    pub fn poll(&self) -> Vec<(SensorId, SensorReading)> {
        let mut pio = match PortIo::open() {
            Some(p) => p,
            None => return Vec::new(),
        };

        let mut readings = Vec::new();
        let chip_name = format!("{}", self.chip.chip).to_lowercase();

        self.read_voltages(&mut pio, &chip_name, &mut readings);
        self.read_temperatures(&mut pio, &chip_name, &mut readings);
        self.read_fans(&mut pio, &chip_name, &mut readings);

        readings
    }

    fn read_voltages(
        &self,
        pio: &mut PortIo,
        chip_name: &str,
        readings: &mut Vec<(SensorId, SensorReading)>,
    ) {
        for (i, &reg) in VOLTAGE_REGS.iter().enumerate() {
            if let Some(raw) = self.read_reg(pio, reg) {
                if raw == 0 {
                    continue;
                }

                // Convert raw ADC to millivolts
                let mv = if self.has_12mv_adc {
                    // 10.9 mV per LSB (kernel: raw * 109 / 10)
                    raw as f64 * 10.9
                } else {
                    // 16 mV per LSB
                    raw as f64 * 16.0
                };

                // Apply internal 2x scaling for AVCC/VSB/Vbat channels
                let mv = if SCALED_CHANNELS.contains(&i) {
                    mv * 2.0
                } else {
                    mv
                };

                let volts = mv / 1000.0;

                let id = SensorId {
                    source: "superio".into(),
                    chip: chip_name.into(),
                    sensor: format!("vin{i}"),
                };
                readings.push((
                    id,
                    SensorReading::new(
                        VOLTAGE_LABELS[i].to_string(),
                        volts,
                        SensorUnit::Volts,
                        SensorCategory::Voltage,
                    ),
                ));
            }
        }
    }

    fn read_temperatures(
        &self,
        pio: &mut PortIo,
        chip_name: &str,
        readings: &mut Vec<(SensorId, SensorReading)>,
    ) {
        for (i, &reg) in TEMP_REGS.iter().enumerate() {
            if let Some(raw) = self.read_reg(pio, reg) {
                let temp = raw as i8 as f64;
                if temp == 0.0 || temp == -128.0 {
                    continue;
                }

                let id = SensorId {
                    source: "superio".into(),
                    chip: chip_name.into(),
                    sensor: format!("temp{}", i + 1),
                };
                readings.push((
                    id,
                    SensorReading::new(
                        TEMP_LABELS[i].to_string(),
                        temp,
                        SensorUnit::Celsius,
                        SensorCategory::Temperature,
                    ),
                ));
            }
        }
    }

    fn read_fans(
        &self,
        pio: &mut PortIo,
        chip_name: &str,
        readings: &mut Vec<(SensorId, SensorReading)>,
    ) {
        // Check if 16-bit fan mode is enabled
        let fan16_enable = self.read_reg(pio, REG_FAN_16BIT).unwrap_or(0);

        for (i, &(fanx_reg, fan_reg)) in FANX_REGS.iter().enumerate() {
            let id = SensorId {
                source: "superio".into(),
                chip: chip_name.into(),
                sensor: format!("fan{}", i + 1),
            };

            let use_16bit = (fan16_enable & (1 << i)) != 0 || i >= 3;

            let count = if use_16bit {
                // 16-bit: FANX has low byte, FAN has high byte
                let lo = self.read_reg(pio, fanx_reg).unwrap_or(0) as u16;
                let hi = self.read_reg(pio, fan_reg).unwrap_or(0) as u16;
                (hi << 8) | lo
            } else {
                // 8-bit mode: just read FAN register
                self.read_reg(pio, fan_reg).unwrap_or(0) as u16
            };

            if count == 0 || count == 0xFFFF {
                readings.push((
                    id,
                    SensorReading::new(
                        FAN_LABELS[i].to_string(),
                        0.0,
                        SensorUnit::Rpm,
                        SensorCategory::Fan,
                    ),
                ));
                continue;
            }

            let rpm = FAN_RPM_FACTOR / count as u32;
            readings.push((
                id,
                SensorReading::new(
                    FAN_LABELS[i].to_string(),
                    rpm as f64,
                    SensorUnit::Rpm,
                    SensorCategory::Fan,
                ),
            ));
        }
    }

    /// Read a single byte from an HWM register (no banking, direct offset).
    fn read_reg(&self, pio: &mut PortIo, reg: u8) -> Option<u8> {
        pio.write_byte(self.addr_port, reg).ok()?;
        pio.read_byte(self.data_port).ok()
    }
}

impl crate::sensors::SensorSource for Ite87xxSource {
    fn name(&self) -> &str {
        "superio"
    }

    fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
        Ite87xxSource::poll(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_voltage_12mv_adc() {
        // IT8686E: raw=200, 10.9 mV per LSB
        let raw = 200u8;
        let mv = raw as f64 * 10.9;
        let v = mv / 1000.0;
        assert!((v - 2.18).abs() < 0.01);
    }

    #[test]
    fn test_voltage_16mv_adc() {
        // IT8689E: raw=200, 16 mV per LSB
        let raw = 200u8;
        let mv = raw as f64 * 16.0;
        let v = mv / 1000.0;
        assert!((v - 3.2).abs() < 0.01);
    }

    #[test]
    fn test_scaled_channel() {
        // AVCC (in3) with 2x internal scaling
        let raw = 150u8;
        let mv = raw as f64 * 10.9 * 2.0;
        let v = mv / 1000.0;
        assert!((v - 3.27).abs() < 0.01);
    }

    #[test]
    fn test_fan_rpm() {
        let count = 675u32;
        let rpm = FAN_RPM_FACTOR / count;
        assert_eq!(rpm, 2000);
    }

    #[test]
    fn test_voltage_reg_count() {
        assert_eq!(VOLTAGE_REGS.len(), VOLTAGE_LABELS.len());
    }

    #[test]
    fn test_temp_reg_count() {
        assert_eq!(TEMP_REGS.len(), TEMP_LABELS.len());
    }

    #[test]
    fn test_fan_reg_count() {
        assert_eq!(FANX_REGS.len(), FAN_LABELS.len());
    }
}
