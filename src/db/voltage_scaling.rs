//! Board-specific voltage scaling for Super I/O chips.
//!
//! Super I/O chips like the NCT6798 measure voltages via ADC inputs. The chip's
//! internal scaling converts raw ADC values to millivolts at the chip's pins.
//! However, motherboards use external resistor dividers to scale high-voltage
//! rails (e.g., +12V, +5V) down to the chip's ADC range. This database provides
//! the external multipliers needed to recover the actual rail voltages.
//!
//! Without board-specific scaling, voltages like +12V and +5V appear as ~1V
//! (their divided-down values at the chip).

/// Voltage channel configuration for a single ADC input.
#[derive(Debug, Clone, Copy)]
pub struct VoltageChannel {
    /// Human-readable label for this channel.
    pub label: &'static str,
    /// External resistor divider multiplier: actual_voltage = chip_voltage * multiplier.
    /// 1.0 means no external divider (chip reads the actual voltage).
    pub multiplier: f64,
}

impl VoltageChannel {
    const fn new(label: &'static str, multiplier: f64) -> Self {
        Self { label, multiplier }
    }

    /// Channel with no external divider (multiplier = 1.0).
    const fn direct(label: &'static str) -> Self {
        Self::new(label, 1.0)
    }
}

/// Look up voltage scaling configuration for a given board and chip.
///
/// Returns a slice of 18 `VoltageChannel` entries (one per NCT6798 ADC input)
/// if a matching board configuration is found, or `None` for unknown boards.
pub fn lookup_nct6798(board_name: Option<&str>) -> Option<&'static [VoltageChannel; 18]> {
    let board = board_name?;
    let lower = board.to_lowercase();

    if lower.contains("wrx90e") {
        Some(&ASUS_WRX90E_SAGE)
    } else if lower.contains("crosshair") && lower.contains("x670") {
        Some(&ASUS_CROSSHAIR_X670E)
    } else if lower.contains("strix") && lower.contains("x670") {
        Some(&ASUS_STRIX_X670E)
    } else if lower.contains("tuf") && lower.contains("x670") {
        Some(&ASUS_TUF_X670E)
    } else {
        None
    }
}

/// Return the default (generic) NCT6798 voltage channel labels.
/// No external multipliers — shows chip-level voltages.
pub fn default_nct6798() -> &'static [VoltageChannel; 18] {
    &DEFAULT_NCT6798
}

// ---------------------------------------------------------------------------
// Default (unknown board): generic labels, no external multipliers
// ---------------------------------------------------------------------------
static DEFAULT_NCT6798: [VoltageChannel; 18] = [
    VoltageChannel::direct("VIN0"),  // 0
    VoltageChannel::direct("VIN1"),  // 1
    VoltageChannel::direct("VIN2"),  // 2
    VoltageChannel::direct("VIN3"),  // 3
    VoltageChannel::direct("VIN4"),  // 4
    VoltageChannel::direct("VIN5"),  // 5
    VoltageChannel::direct("VIN6"),  // 6
    VoltageChannel::direct("VIN7"),  // 7
    VoltageChannel::direct("VBAT"),  // 8
    VoltageChannel::direct("VTT"),   // 9
    VoltageChannel::direct("VIN10"), // 10
    VoltageChannel::direct("VIN11"), // 11
    VoltageChannel::direct("VIN12"), // 12
    VoltageChannel::direct("VIN13"), // 13
    VoltageChannel::direct("VIN14"), // 14
    VoltageChannel::direct("VIN15"), // 15
    VoltageChannel::direct("VIN16"), // 16
    VoltageChannel::direct("VIN17"), // 17
];

// ---------------------------------------------------------------------------
// ASUS Pro WS WRX90E-SAGE SE (AMD TRX50 / WRX90 chipset, NCT6798D)
// ---------------------------------------------------------------------------
static ASUS_WRX90E_SAGE: [VoltageChannel; 18] = [
    VoltageChannel::direct("Vcore"), // VIN0: CPU core voltage (~0.8-1.4V)
    VoltageChannel::new("+5V", 5.0), // VIN1: +5V rail through 4:1 divider
    VoltageChannel::direct("AVCC"),  // VIN2: +3.3V (internal 2:1, scale=1600)
    VoltageChannel::direct("+3.3V Standby"), // VIN3: +3.3V standby (scale=1600)
    VoltageChannel::new("+12V", 12.0), // VIN4: +12V rail through 11:1 divider
    VoltageChannel::direct("VIN5"),  // VIN5: unknown rail (~1V)
    VoltageChannel::direct("VIN6"),  // VIN6: unknown rail (~0.6V)
    VoltageChannel::direct("+3.3V AUX"), // VIN7: +3.3V auxiliary (scale=1600)
    VoltageChannel::direct("Vbat"),  // VIN8: CMOS battery (~3.0V, scale=1600)
    VoltageChannel::direct("VTT"),   // VIN9: DDR termination (scale=1600)
    VoltageChannel::direct("VIN10"), // VIN10
    VoltageChannel::direct("VIN11"), // VIN11
    VoltageChannel::direct("VDDSOC"), // VIN12: SoC voltage (~1.0V)
    VoltageChannel::direct("VIN13"), // VIN13
    VoltageChannel::direct("VIN14"), // VIN14
    VoltageChannel::direct("VIN15"), // VIN15
    VoltageChannel::direct("VIN16"), // VIN16
    VoltageChannel::direct("VIN17"), // VIN17
];

// ---------------------------------------------------------------------------
// ASUS ROG CROSSHAIR X670E HERO (AMD AM5, NCT6798D)
// Based on LibreHardwareMonitor Nct677X.cs patterns for ASUS AM5 boards
// ---------------------------------------------------------------------------
static ASUS_CROSSHAIR_X670E: [VoltageChannel; 18] = [
    VoltageChannel::direct("Vcore"),     // VIN0
    VoltageChannel::new("+5V", 5.0),     // VIN1
    VoltageChannel::direct("AVCC"),      // VIN2
    VoltageChannel::direct("+3.3V"),     // VIN3
    VoltageChannel::new("+12V", 12.0),   // VIN4
    VoltageChannel::direct("VIN5"),      // VIN5
    VoltageChannel::direct("VIN6"),      // VIN6
    VoltageChannel::direct("+3.3V AUX"), // VIN7
    VoltageChannel::direct("Vbat"),      // VIN8
    VoltageChannel::direct("VTT"),       // VIN9
    VoltageChannel::direct("VIN10"),     // VIN10
    VoltageChannel::direct("VIN11"),     // VIN11
    VoltageChannel::direct("VIN12"),     // VIN12
    VoltageChannel::direct("VIN13"),     // VIN13
    VoltageChannel::direct("VIN14"),     // VIN14
    VoltageChannel::direct("VIN15"),     // VIN15
    VoltageChannel::direct("VIN16"),     // VIN16
    VoltageChannel::direct("VIN17"),     // VIN17
];

// ---------------------------------------------------------------------------
// ASUS ROG STRIX X670E-E GAMING WIFI (AMD AM5, NCT6798D)
// ---------------------------------------------------------------------------
static ASUS_STRIX_X670E: [VoltageChannel; 18] = [
    VoltageChannel::direct("Vcore"),     // VIN0
    VoltageChannel::new("+5V", 5.0),     // VIN1
    VoltageChannel::direct("AVCC"),      // VIN2
    VoltageChannel::direct("+3.3V"),     // VIN3
    VoltageChannel::new("+12V", 12.0),   // VIN4
    VoltageChannel::direct("VIN5"),      // VIN5
    VoltageChannel::direct("VIN6"),      // VIN6
    VoltageChannel::direct("+3.3V AUX"), // VIN7
    VoltageChannel::direct("Vbat"),      // VIN8
    VoltageChannel::direct("VTT"),       // VIN9
    VoltageChannel::direct("VIN10"),     // VIN10
    VoltageChannel::direct("VIN11"),     // VIN11
    VoltageChannel::direct("VIN12"),     // VIN12
    VoltageChannel::direct("VIN13"),     // VIN13
    VoltageChannel::direct("VIN14"),     // VIN14
    VoltageChannel::direct("VIN15"),     // VIN15
    VoltageChannel::direct("VIN16"),     // VIN16
    VoltageChannel::direct("VIN17"),     // VIN17
];

// ---------------------------------------------------------------------------
// ASUS TUF GAMING X670E-PLUS (AMD AM5, NCT6798D)
// ---------------------------------------------------------------------------
static ASUS_TUF_X670E: [VoltageChannel; 18] = [
    VoltageChannel::direct("Vcore"),     // VIN0
    VoltageChannel::new("+5V", 5.0),     // VIN1
    VoltageChannel::direct("AVCC"),      // VIN2
    VoltageChannel::direct("+3.3V"),     // VIN3
    VoltageChannel::new("+12V", 12.0),   // VIN4
    VoltageChannel::direct("VIN5"),      // VIN5
    VoltageChannel::direct("VIN6"),      // VIN6
    VoltageChannel::direct("+3.3V AUX"), // VIN7
    VoltageChannel::direct("Vbat"),      // VIN8
    VoltageChannel::direct("VTT"),       // VIN9
    VoltageChannel::direct("VIN10"),     // VIN10
    VoltageChannel::direct("VIN11"),     // VIN11
    VoltageChannel::direct("VIN12"),     // VIN12
    VoltageChannel::direct("VIN13"),     // VIN13
    VoltageChannel::direct("VIN14"),     // VIN14
    VoltageChannel::direct("VIN15"),     // VIN15
    VoltageChannel::direct("VIN16"),     // VIN16
    VoltageChannel::direct("VIN17"),     // VIN17
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lookup_wrx90e() {
        let config = lookup_nct6798(Some("Pro WS WRX90E-SAGE SE"));
        assert!(config.is_some());
        let channels = config.unwrap();
        assert_eq!(channels[0].label, "Vcore");
        assert_eq!(channels[0].multiplier, 1.0);
        assert_eq!(channels[1].label, "+5V");
        assert_eq!(channels[1].multiplier, 5.0);
        assert_eq!(channels[4].label, "+12V");
        assert_eq!(channels[4].multiplier, 12.0);
    }

    #[test]
    fn test_lookup_crosshair() {
        let config = lookup_nct6798(Some("ROG CROSSHAIR X670E HERO"));
        assert!(config.is_some());
        assert_eq!(config.unwrap()[0].label, "Vcore");
    }

    #[test]
    fn test_lookup_unknown_board() {
        assert!(lookup_nct6798(Some("Unknown Board XYZ")).is_none());
    }

    #[test]
    fn test_lookup_none() {
        assert!(lookup_nct6798(None).is_none());
    }

    #[test]
    fn test_default_has_no_multipliers() {
        let def = default_nct6798();
        for ch in def.iter() {
            assert_eq!(ch.multiplier, 1.0);
        }
    }

    #[test]
    fn test_voltage_scaling_calculation() {
        // Raw ADC value 125, chip scale 800 → 1000 mV at chip
        // With +12V multiplier of 12.0 → 12000 mV = 12.0V
        let raw = 125u8;
        let chip_scale = 800u32;
        let multiplier = 12.0;
        let chip_mv = raw as f64 * chip_scale as f64 / 100.0;
        let actual_mv = chip_mv * multiplier;
        let actual_v = actual_mv / 1000.0;
        assert!((actual_v - 12.0).abs() < 0.1);
    }
}
