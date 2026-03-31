//! Direct register reader for ITE IT86xxE/IT87xxE Super I/O chips.
//!
//! Common on Gigabyte and ASUS motherboards. Reads voltages, temperatures, and
//! fan speeds directly from the chip's hardware monitoring registers via I/O
//! ports. Requires root or CAP_SYS_RAWIO.
//!
//! Register layout from ITE register map and datasheets.

use crate::model::sensor::{SensorCategory, SensorId, SensorReading, SensorUnit};
use crate::platform::port_io::PortIo;
use crate::sensors::superio::chip_detect::{ChipType, SuperIoChip};

// Voltage input registers (direct offset, no banking needed).
// First 10 (VIN0-VIN9) are always voltage inputs. VIN10-12 (0x2C-0x2E)
// share registers with TEMP4-6 and are only read when register 0x77
// indicates they are configured as voltage inputs.
const VOLTAGE_REGS_BASE: [u8; 10] = [0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x2F];
const VOLTAGE_REGS_SHARED: [u8; 3] = [0x2C, 0x2D, 0x2E]; // VIN10/11/12 aka TEMP4/5/6

const VOLTAGE_LABELS: [&str; 13] = [
    "VIN0", "VIN1", "VIN2", "AVCC", "VIN4", "VIN5", "VIN6", "VSB", "Vbat", "VIN9", "VIN10",
    "VIN11", "VIN12",
];

// Channels with internal 2x scaling (voltage divider in the chip)
const SCALED_CHANNELS: [usize; 4] = [3, 7, 8, 9];

// Temperature registers: first 3 are universal. TEMP4-6 (0x2C-0x2E) share
// registers with VIN10-12 — only read when register 0x77 indicates temp mode,
// or unconditionally on IT8655/IT8665 (always temp on those chips).
const TEMP_REGS_BASE: [u8; 3] = [0x29, 0x2A, 0x2B];
const TEMP_REGS_SHARED: [u8; 3] = [0x2C, 0x2D, 0x2E]; // TEMP4/5/6 aka VIN10/11/12
const TEMP_LABELS: [&str; 6] = ["Temp 1", "Temp 2", "Temp 3", "Temp 4", "Temp 5", "Temp 6"];

// Register that controls whether 0x2C/0x2D/0x2E act as temps or voltages.
// Each channel uses a 2-bit field: >=2 means temp, ==1 means voltage.
const REG_TEMP456_ENABLE: u8 = 0x77;

// Fan tachometer registers (16-bit extended, up to 6 fans)
// Fans 1-3 are universal, 4-6 vary by chip
const FANX_REGS: [(u8, u8); 5] = [
    (0x18, 0x0D), // Fan 1: FANX=0x18, FAN=0x0D
    (0x19, 0x0E), // Fan 2
    (0x1A, 0x0F), // Fan 3
    (0x81, 0x80), // Fan 4
    (0x83, 0x82), // Fan 5
];
// Fan 6 registers differ by chip family
const FAN6_REGS_STANDARD: (u8, u8) = (0x4D, 0x4C);
const FAN6_REGS_IT8665: (u8, u8) = (0x94, 0x93);

const FAN_LABELS: [&str; 6] = ["Fan 1", "Fan 2", "Fan 3", "Fan 4", "Fan 5", "Fan 6"];

// RPM calculation constant (same as Nuvoton)
const FAN_RPM_FACTOR: u32 = 1_350_000;

// 16-bit fan enable register
const REG_FAN_16BIT: u8 = 0x0C;

pub struct Ite87xxSource {
    chip: SuperIoChip,
    addr_port: u16,
    data_port: u16,
    /// ADC millivolts per LSB — varies by chip family.
    adc_mv: f64,
}

/// ADC millivolts per LSB, per chip family.
fn adc_mv_per_lsb(chip: ChipType) -> f64 {
    match chip {
        ChipType::Ite8655 | ChipType::Ite8665 => 10.9,
        ChipType::Ite8613 | ChipType::Ite8792 | ChipType::Ite8695 => 11.0,
        // IT8628, IT8686, IT8688, IT8689, IT8696 all use 12mV
        _ => 12.0,
    }
}

impl Ite87xxSource {
    pub fn new(chip: SuperIoChip) -> Self {
        let addr_port = chip.hwm_base + 5;
        let data_port = chip.hwm_base + 6;
        let adc_mv = adc_mv_per_lsb(chip.chip);
        Self {
            chip,
            addr_port,
            data_port,
            adc_mv,
        }
    }

    pub fn is_supported(&self) -> bool {
        matches!(
            self.chip.chip,
            ChipType::Ite8613
                | ChipType::Ite8628
                | ChipType::Ite8655
                | ChipType::Ite8665
                | ChipType::Ite8686
                | ChipType::Ite8688
                | ChipType::Ite8689
                | ChipType::Ite8695
                | ChipType::Ite8696
                | ChipType::Ite8792
        )
    }

    /// Whether this chip supports extended temp/voltage channels (0x2C-0x2E).
    /// IT8655/IT8665 always use them as temps. Other chips with 6 temps
    /// need register 0x77 checked. Chips with only 3 temps don't have them.
    fn has_extended_channels(&self) -> bool {
        !matches!(
            self.chip.chip,
            ChipType::Ite8792 | ChipType::Ite8695 | ChipType::Ite8613
        )
    }

    /// IT8655/IT8665 always treat 0x2C-0x2E as temps (no 0x77 check needed).
    fn always_temp456(&self) -> bool {
        matches!(self.chip.chip, ChipType::Ite8655 | ChipType::Ite8665)
    }

    /// Number of base fan channels (excludes fan 6 which has chip-specific regs).
    fn num_base_fans(&self) -> usize {
        match self.chip.chip {
            ChipType::Ite8792 | ChipType::Ite8695 | ChipType::Ite8655 => 3,
            ChipType::Ite8613 => 5, // has fans 2-5 (fan 1 skipped below)
            _ => 5,                 // fans 1-5, fan 6 handled separately
        }
    }

    /// Whether this chip has a fan 6 channel.
    fn has_fan6(&self) -> bool {
        matches!(
            self.chip.chip,
            ChipType::Ite8628
                | ChipType::Ite8665
                | ChipType::Ite8686
                | ChipType::Ite8688
                | ChipType::Ite8689
                | ChipType::Ite8696
        )
    }

    /// Fan 6 register addresses (chip-family-dependent).
    fn fan6_regs(&self) -> (u8, u8) {
        match self.chip.chip {
            ChipType::Ite8665 => FAN6_REGS_IT8665,
            _ => FAN6_REGS_STANDARD,
        }
    }

    /// Whether fan at index `i` should be skipped (IT8613 has no fan 1).
    fn skip_fan(&self, i: usize) -> bool {
        i == 0 && self.chip.chip == ChipType::Ite8613
    }

    pub fn poll(&self) -> Vec<(SensorId, SensorReading)> {
        let mut pio = match PortIo::open() {
            Some(p) => p,
            None => return Vec::new(),
        };

        let mut readings = Vec::new();
        let chip_name = format!("{}", self.chip.chip).to_lowercase();

        // Probe shared register mode once per poll cycle
        let shared_is_voltage = self.probe_shared_regs(&mut pio);

        self.read_voltages(&mut pio, &chip_name, &shared_is_voltage, &mut readings);
        self.read_temperatures(&mut pio, &chip_name, &shared_is_voltage, &mut readings);
        self.read_fans(&mut pio, &chip_name, &mut readings);

        readings
    }

    /// Determine which of registers 0x2C/0x2D/0x2E are configured as voltage
    /// inputs (vs temperature). Returns a 3-element array: true = voltage mode.
    fn probe_shared_regs(&self, pio: &mut PortIo) -> [bool; 3] {
        if !self.has_extended_channels() {
            // IT8792/IT8695/IT8613: no shared channels
            return [false; 3];
        }
        if self.always_temp456() {
            // IT8655/IT8665: always temp mode, never voltage
            return [false; 3];
        }
        // Read register 0x77 to determine per-channel mode.
        // Each channel uses a 2-bit field: ==1 means voltage, >=2 means temp.
        let reg77 = self.read_reg(pio, REG_TEMP456_ENABLE).unwrap_or(0);
        [
            (reg77 & 0x03) == 0x01,      // 0x2C: bits [1:0]
            (reg77 >> 2 & 0x03) == 0x01, // 0x2D: bits [3:2]
            (reg77 >> 4 & 0x03) == 0x01, // 0x2E: bits [5:4]
        ]
    }

    fn read_voltage(
        &self,
        pio: &mut PortIo,
        chip_name: &str,
        idx: usize,
        reg: u8,
        readings: &mut Vec<(SensorId, SensorReading)>,
    ) {
        let Some(raw) = self.read_reg(pio, reg) else {
            return;
        };
        if raw == 0 {
            return;
        }

        let mut mv = raw as f64 * self.adc_mv;

        // Apply internal 2x scaling for AVCC/VSB/Vbat channels
        if SCALED_CHANNELS.contains(&idx) {
            mv *= 2.0;
        }

        let id = SensorId {
            source: "superio".into(),
            chip: chip_name.into(),
            sensor: format!("vin{idx}"),
        };
        readings.push((
            id,
            SensorReading::new(
                VOLTAGE_LABELS[idx].to_string(),
                mv / 1000.0,
                SensorUnit::Volts,
                SensorCategory::Voltage,
            ),
        ));
    }

    fn read_voltages(
        &self,
        pio: &mut PortIo,
        chip_name: &str,
        shared_is_voltage: &[bool; 3],
        readings: &mut Vec<(SensorId, SensorReading)>,
    ) {
        // Always read base voltage channels (VIN0-VIN9)
        for (i, &reg) in VOLTAGE_REGS_BASE.iter().enumerate() {
            self.read_voltage(pio, chip_name, i, reg, readings);
        }

        // Only read VIN10-12 when the shared registers are in voltage mode
        for (j, &reg) in VOLTAGE_REGS_SHARED.iter().enumerate() {
            if shared_is_voltage[j] {
                self.read_voltage(pio, chip_name, 10 + j, reg, readings);
            }
        }
    }

    fn read_temp(
        &self,
        pio: &mut PortIo,
        chip_name: &str,
        idx: usize,
        reg: u8,
        readings: &mut Vec<(SensorId, SensorReading)>,
    ) {
        let Some(raw) = self.read_reg(pio, reg) else {
            return;
        };
        let temp = raw as i8 as f64;
        if temp == 0.0 || temp == -128.0 {
            return;
        }

        let id = SensorId {
            source: "superio".into(),
            chip: chip_name.into(),
            sensor: format!("temp{}", idx + 1),
        };
        readings.push((
            id,
            SensorReading::new(
                TEMP_LABELS[idx].to_string(),
                temp,
                SensorUnit::Celsius,
                SensorCategory::Temperature,
            ),
        ));
    }

    fn read_temperatures(
        &self,
        pio: &mut PortIo,
        chip_name: &str,
        shared_is_voltage: &[bool; 3],
        readings: &mut Vec<(SensorId, SensorReading)>,
    ) {
        // Always read base temp channels (TEMP1-3)
        for (i, &reg) in TEMP_REGS_BASE.iter().enumerate() {
            self.read_temp(pio, chip_name, i, reg, readings);
        }

        // Read TEMP4-6 only when chip supports them and they're not in voltage mode
        if self.has_extended_channels() {
            for (j, &reg) in TEMP_REGS_SHARED.iter().enumerate() {
                if !shared_is_voltage[j] {
                    self.read_temp(pio, chip_name, 3 + j, reg, readings);
                }
            }
        }
    }

    fn read_fans(
        &self,
        pio: &mut PortIo,
        chip_name: &str,
        readings: &mut Vec<(SensorId, SensorReading)>,
    ) {
        let fan16_enable = self.read_reg(pio, REG_FAN_16BIT).unwrap_or(0);
        let num_base = self.num_base_fans();

        // Base fans (1-5) + optional fan 6 without heap allocation
        for (idx, fanx_reg, fan_reg) in FANX_REGS[..num_base]
            .iter()
            .enumerate()
            .map(|(i, &(fx, f))| (i, fx, f))
            .chain(
                self.has_fan6()
                    .then_some(self.fan6_regs())
                    .map(|(fx, f)| (5, fx, f)),
            )
        {
            if self.skip_fan(idx) {
                continue;
            }

            let use_16bit = (fan16_enable & (1 << idx)) != 0 || idx >= 3;
            let count = if use_16bit {
                let lo = self.read_reg(pio, fanx_reg).unwrap_or(0) as u16;
                let hi = self.read_reg(pio, fan_reg).unwrap_or(0) as u16;
                (hi << 8) | lo
            } else {
                self.read_reg(pio, fan_reg).unwrap_or(0) as u16
            };

            let rpm = if count == 0 || count == 0xFFFF {
                0.0
            } else {
                (FAN_RPM_FACTOR / count as u32) as f64
            };

            let id = SensorId {
                source: "superio".into(),
                chip: chip_name.into(),
                sensor: format!("fan{}", idx + 1),
            };
            readings.push((
                id,
                SensorReading::new(
                    FAN_LABELS[idx].to_string(),
                    rpm,
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
    fn test_adc_resolution_per_chip() {
        assert!((adc_mv_per_lsb(ChipType::Ite8655) - 10.9).abs() < 0.01);
        assert!((adc_mv_per_lsb(ChipType::Ite8665) - 10.9).abs() < 0.01);
        assert!((adc_mv_per_lsb(ChipType::Ite8613) - 11.0).abs() < 0.01);
        assert!((adc_mv_per_lsb(ChipType::Ite8792) - 11.0).abs() < 0.01);
        assert!((adc_mv_per_lsb(ChipType::Ite8695) - 11.0).abs() < 0.01);
        assert!((adc_mv_per_lsb(ChipType::Ite8628) - 12.0).abs() < 0.01);
        assert!((adc_mv_per_lsb(ChipType::Ite8686) - 12.0).abs() < 0.01);
        assert!((adc_mv_per_lsb(ChipType::Ite8688) - 12.0).abs() < 0.01);
        assert!((adc_mv_per_lsb(ChipType::Ite8689) - 12.0).abs() < 0.01);
        assert!((adc_mv_per_lsb(ChipType::Ite8696) - 12.0).abs() < 0.01);
    }

    #[test]
    fn test_voltage_12mv_adc() {
        // IT8686E: raw=200, 12.0 mV per LSB
        let raw = 200u8;
        let mv = raw as f64 * 12.0;
        let v = mv / 1000.0;
        assert!((v - 2.4).abs() < 0.01);
    }

    #[test]
    fn test_scaled_channel() {
        // AVCC (in3) with 2x internal scaling, 12mV chip
        let raw = 150u8;
        let mv = raw as f64 * 12.0 * 2.0;
        let v = mv / 1000.0;
        assert!((v - 3.6).abs() < 0.01);
    }

    #[test]
    fn test_fan_rpm() {
        let count = 675u32;
        let rpm = FAN_RPM_FACTOR / count;
        assert_eq!(rpm, 2000);
    }

    #[test]
    fn test_register_counts() {
        assert_eq!(
            VOLTAGE_REGS_BASE.len() + VOLTAGE_REGS_SHARED.len(),
            VOLTAGE_LABELS.len()
        );
        assert_eq!(
            TEMP_REGS_BASE.len() + TEMP_REGS_SHARED.len(),
            TEMP_LABELS.len()
        );
        // 5 base fan regs + fan 6 handled separately = 6 labels
        assert_eq!(FANX_REGS.len() + 1, FAN_LABELS.len());
    }
}
