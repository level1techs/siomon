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
    let template = super::boards::lookup_board(board)?;
    template.nct_voltage_scaling
}

/// Return the default (generic) NCT6798 voltage channel labels.
/// No external multipliers — shows chip-level voltages.
pub fn default_nct6798() -> &'static [VoltageChannel; 18] {
    &DEFAULT_NCT6798
}

/// Look up ITE87xx voltage scaling configuration for a given board.
///
/// Returns a slice of 13 `VoltageChannel` entries (one per ITE ADC input)
/// if a matching board configuration is found, or `None` for unknown boards.
pub fn lookup_ite87xx(board_name: Option<&str>) -> Option<&'static [VoltageChannel; 13]> {
    let board = board_name?;
    let template = super::boards::lookup_board(board)?;
    template.ite_voltage_scaling
}

/// Return the default (generic) ITE87xx voltage channel labels.
/// No external multipliers — shows chip-level voltages.
pub fn default_ite87xx() -> &'static [VoltageChannel; 13] {
    &DEFAULT_ITE87XX
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
// Default (unknown board): generic ITE labels, no external multipliers
// ---------------------------------------------------------------------------
static DEFAULT_ITE87XX: [VoltageChannel; 13] = [
    VoltageChannel::direct("VIN0"),  // 0
    VoltageChannel::direct("VIN1"),  // 1
    VoltageChannel::direct("VIN2"),  // 2
    VoltageChannel::direct("AVCC"),  // 3 (internal 2x scaled)
    VoltageChannel::direct("VIN4"),  // 4
    VoltageChannel::direct("VIN5"),  // 5
    VoltageChannel::direct("VIN6"),  // 6
    VoltageChannel::direct("VSB"),   // 7 (internal 2x scaled)
    VoltageChannel::direct("Vbat"),  // 8 (internal 2x scaled)
    VoltageChannel::direct("VIN9"),  // 9 (internal 2x scaled)
    VoltageChannel::direct("VIN10"), // 10
    VoltageChannel::direct("VIN11"), // 11
    VoltageChannel::direct("VIN12"), // 12
];

// ---------------------------------------------------------------------------
// Gigabyte TRX50 AI TOP (AMD TRX50, IT8689E + IT87952E)
// Voltage dividers for the primary IT8689E chip.
// +3.3V on VIN1: 33/20 divider → multiplier 1.65
// +12V on VIN2: 120/20 divider → multiplier 6.0
// +5V on VIN3 (AVCC): internal 2x gives 4.0V reported, actual 4.93V probed
//   → external multiplier 4.93 / 4.0 = 1.2325
// ---------------------------------------------------------------------------
pub static GIGABYTE_TRX50_AI_TOP_ITE: [VoltageChannel; 13] = [
    VoltageChannel::direct("Vcore"),     // VIN0: CPU core (~1.4V)
    VoltageChannel::new("+3.3V", 1.65),  // VIN1: +3.3V rail
    VoltageChannel::new("+12V", 6.0),    // VIN2: +12V rail
    VoltageChannel::new("+5V", 1.2325),  // VIN3: +5V rail (AVCC, internal 2x + ext divider)
    VoltageChannel::direct("Vcore SoC"), // VIN4: SoC voltage
    VoltageChannel::direct("CPU VDDP"),  // VIN5: CPU VDDP
    VoltageChannel::direct("DRAM"),      // VIN6: DRAM voltage
    VoltageChannel::direct("+3.3V Standby"), // VIN7: +3.3V standby (internal 2x)
    VoltageChannel::direct("Vbat"),      // VIN8: CMOS battery (internal 2x)
    VoltageChannel::direct("VIN9"),      // VIN9
    VoltageChannel::direct("VIN10"),     // VIN10
    VoltageChannel::direct("VIN11"),     // VIN11
    VoltageChannel::direct("VIN12"),     // VIN12
];

// ---------------------------------------------------------------------------
// ASUS Pro WS WRX90E-SAGE SE (AMD TRX50 / WRX90 chipset, NCT6798D)
// ---------------------------------------------------------------------------
pub static ASUS_WRX90E_SAGE: [VoltageChannel; 18] = [
    VoltageChannel::direct("Vcore"), // VIN0: CPU core voltage (~0.8-1.4V)
    VoltageChannel::new("+5V", 5.0), // VIN1: +5V rail through 5:1 divider (Ri=4k, Rf=1k)
    VoltageChannel::direct("AVCC"),  // VIN2: +3.3V (internal 2:1, scale=1600)
    VoltageChannel::direct("+3.3V Standby"), // VIN3: +3.3V standby (scale=1600)
    VoltageChannel::new("+12V", 12.0), // VIN4: +12V rail through 12:1 divider (Ri=11k, Rf=1k)
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
// ASUS Pro WS TRX50-SAGE WIFI A (AMD TRX50, NCT6799D)
// Calibrated from BIOS Monitor page and direct-io readings.
// ---------------------------------------------------------------------------
pub static ASUS_TRX50_SAGE: [VoltageChannel; 18] = [
    VoltageChannel::direct("CPU Core0"),      // VIN0
    VoltageChannel::new("+5V", 5.0),          // VIN1
    VoltageChannel::direct("AVCC"),           // VIN2
    VoltageChannel::direct("+3.3V"),          // VIN3
    VoltageChannel::new("+12V", 12.0),        // VIN4
    VoltageChannel::direct("VIN5"),           // VIN5
    VoltageChannel::direct("VIN6"),           // VIN6
    VoltageChannel::direct("VIN7"),           // VIN7
    VoltageChannel::direct("Vbat"),           // VIN8
    VoltageChannel::direct("VTT"),            // VIN9
    VoltageChannel::direct("CPU VDDIO"),      // VIN10
    VoltageChannel::direct("VIN11"),          // VIN11
    VoltageChannel::direct("VDD_11_S3 / MC"), // VIN12
    VoltageChannel::direct("VIN13"),          // VIN13
    VoltageChannel::direct("VIN14"),          // VIN14
    VoltageChannel::direct("CPU VSOC"),       // VIN15
    VoltageChannel::direct("VIN16"),          // VIN16
    VoltageChannel::direct("VIN17"),          // VIN17
];

// ---------------------------------------------------------------------------
// Shared ASUS AM5 NCT6798D voltage scaling (Crosshair/Strix/TUF X670E)
// Based on LibreHardwareMonitor Nct677X.cs patterns for ASUS AM5 boards
// ---------------------------------------------------------------------------
pub static ASUS_AM5_NCT6798: [VoltageChannel; 18] = [
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
// ASUS Pro WS W890E-SAGE SE (Intel W890, NCT6799D)
// Labels/multipliers derived from a live direct-I/O scan on the target board
// and correlated against the BMC/IPMI telemetry exposed by the same host.
// ---------------------------------------------------------------------------
pub static ASUS_W890E_SAGE: [VoltageChannel; 18] = [
    VoltageChannel::direct("Vcore"),         // VIN0: ~0.90V at idle
    VoltageChannel::new("+12V", 12.0),       // VIN1: 1.024V * 12.0 ~= 12.29V
    VoltageChannel::direct("+3.3V Standby"), // VIN2: ~3.41V
    VoltageChannel::direct("+3.3V"),         // VIN3: ~3.44V
    VoltageChannel::direct("VIN4"),          // VIN4: unknown ~1.02V rail
    VoltageChannel::direct("+0.82V PCH"),    // VIN5: ~0.82V
    VoltageChannel::direct("VIN6"),          // VIN6: unknown ~0.91V rail
    VoltageChannel::direct("+3.3V AUX"),     // VIN7: ~3.41V
    VoltageChannel::direct("Vbat"),          // VIN8
    VoltageChannel::direct("VNN MN 1.02V"),  // VIN9: ~0.99-1.00V
    VoltageChannel::new("+5V", 5.7),         // VIN10: 0.896V * 5.7 ~= 5.11V
    VoltageChannel::new("+5V Standby", 5.7), // VIN11: 0.896V * 5.7 ~= 5.11V
    VoltageChannel::direct("VIN12"),         // VIN12: unknown ~1.05V rail
    VoltageChannel::direct("VIN13"),         // VIN13: unknown ~1.69V rail
    VoltageChannel::direct("VIN14"),         // VIN14: unknown ~1.00V rail
    VoltageChannel::direct("VIN15"),         // VIN15: unknown ~0.88V rail
    VoltageChannel::direct("+3.3V CPU"),     // VIN16: ~3.41V
    VoltageChannel::direct("VIN17"),         // VIN17: unknown ~1.00V rail
];

// ---------------------------------------------------------------------------
// ASRock Z890 Nova WiFi (Intel Z890, NCT6798D)
// Labels/multipliers derived from the BIOS H/W Monitor page and validated
// against live direct-I/O readings from the rebuilt debug binary.
// ---------------------------------------------------------------------------
pub static ASROCK_Z890_NOVA: [VoltageChannel; 18] = [
    VoltageChannel::direct("Vcore"),          // VIN0: ~0.85-0.90V at idle
    VoltageChannel::direct("VIN1"),           // VIN1: unstable / unknown
    VoltageChannel::direct("+3.30V"),         // VIN2: ~3.42V
    VoltageChannel::direct("+3.30V Standby"), // VIN3: ~3.38V
    VoltageChannel::direct("+VNNAON"),        // VIN4: 0.768V
    VoltageChannel::direct("VCCSA"),          // VIN5: 1.24V
    VoltageChannel::direct("+0.82V PCH"),     // VIN6: 0.872V
    VoltageChannel::direct("+3.30V AUX"),     // VIN7: ~3.42V
    VoltageChannel::direct("Vbat"),           // VIN8
    VoltageChannel::direct("VTT"),            // VIN9
    VoltageChannel::new("+5.00V", 13.2),      // VIN10: 0.384V * 13.2 = 5.07V
    VoltageChannel::new("+12.00V", 13.5),     // VIN11: 0.896V * 13.5 = 12.096V
    VoltageChannel::direct("VIN12"),          // VIN12: unknown rail
    VoltageChannel::new("ATX5VSB", 8.0),      // VIN13: 0.632V * 8.0 = 5.056V
    VoltageChannel::direct("VDD2"),           // VIN14: 1.512V
    VoltageChannel::direct("VIN15"),          // VIN15: unknown rail
    VoltageChannel::direct("+VCC1.8V"),       // VIN16: 1.84V
    VoltageChannel::direct("VIN17"),          // VIN17
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
    fn test_lookup_asrock_z890_nova() {
        let config = crate::db::boards::lookup_board_with_vendor("Z890 Nova WiFi", "ASRock")
            .and_then(|board| board.nct_voltage_scaling);
        assert!(config.is_some());
        let channels = config.unwrap();
        assert_eq!(channels[10].label, "+5.00V");
        assert_eq!(channels[10].multiplier, 13.2);
        assert_eq!(channels[11].label, "+12.00V");
        assert_eq!(channels[11].multiplier, 13.5);
        assert_eq!(channels[13].label, "ATX5VSB");
        assert_eq!(channels[13].multiplier, 8.0);
    }

    #[test]
    fn test_lookup_asus_w890e_sage() {
        let config = crate::db::boards::lookup_board_with_vendor(
            "Pro WS W890E-SAGE SE",
            "ASUSTeK COMPUTER INC.",
        )
        .and_then(|board| board.nct_voltage_scaling);
        assert!(config.is_some());
        let channels = config.unwrap();
        assert_eq!(channels[0].label, "Vcore");
        assert_eq!(channels[1].label, "+12V");
        assert_eq!(channels[1].multiplier, 12.0);
        assert_eq!(channels[10].label, "+5V");
        assert_eq!(channels[10].multiplier, 5.7);
        assert_eq!(channels[11].label, "+5V Standby");
        assert_eq!(channels[16].label, "+3.3V CPU");
    }

    #[test]
    fn test_lookup_trx50_sage() {
        let config = lookup_nct6798(Some("Pro WS TRX50-SAGE WIFI A"));
        assert!(config.is_some());
        let channels = config.unwrap();
        assert_eq!(channels[1].label, "+5V");
        assert_eq!(channels[1].multiplier, 5.0);
        assert_eq!(channels[4].label, "+12V");
        assert_eq!(channels[4].multiplier, 12.0);
    }

    #[test]
    fn test_lookup_ite87xx_trx50_ai_top() {
        let config = lookup_ite87xx(Some("TRX50 AI TOP"));
        assert!(config.is_some());
        let channels = config.unwrap();
        assert_eq!(channels[1].label, "+3.3V");
        assert!((channels[1].multiplier - 1.65).abs() < 0.01);
        assert_eq!(channels[2].label, "+12V");
        assert!((channels[2].multiplier - 6.0).abs() < 0.01);
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
