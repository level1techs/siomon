use crate::platform::sysfs;
use crate::sensors::i2c::bus_scan::{I2cAdapterType, I2cBus};

use std::path::Path;

/// Explicitly supported boards for direct DDR5 SPD/temperature probing.
///
/// We keep this as a whitelist because probing SMBus/I2C addresses directly is
/// not something we should do on unknown boards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AmdDdr5Board {
    pub model: &'static str,
    pub match_substrings: &'static [&'static str],
    pub designware_buses: &'static [u32],
    pub slots_per_bus: u16,
}

const ASUS_WRX90E_SAGE_SE: AmdDdr5Board = AmdDdr5Board {
    model: "ASUS Pro WS WRX90E-SAGE SE",
    match_substrings: &["pro ws", "wrx90e", "sage"],
    designware_buses: &[1, 2],
    slots_per_bus: 4,
};

const ASROCK_WRX90_WS_EVO: AmdDdr5Board = AmdDdr5Board {
    model: "ASRock WRX90 WS EVO",
    match_substrings: &["wrx90", "ws evo"],
    designware_buses: &[1, 2],
    slots_per_bus: 4,
};

const ASUS_TRX50_SAGE_WIFI_A: AmdDdr5Board = AmdDdr5Board {
    model: "ASUS Pro WS TRX50-SAGE WIFI A",
    match_substrings: &["pro ws", "trx50", "sage"],
    designware_buses: &[0, 1],
    slots_per_bus: 4,
};

const SUPPORTED_BOARDS: &[AmdDdr5Board] = &[
    ASUS_WRX90E_SAGE_SE,
    ASROCK_WRX90_WS_EVO,
    ASUS_TRX50_SAGE_WIFI_A,
];

/// Read the DMI board name used for board-whitelist detection.
pub fn board_name() -> Option<String> {
    sysfs::read_string_optional(Path::new("/sys/class/dmi/id/board_name"))
}

/// Return the supported AMD DDR5 board profile for this host, if any.
pub fn detect_board() -> Option<&'static AmdDdr5Board> {
    let board_name = board_name()?;
    let board = lookup_board(&board_name);
    log::debug!(
        "AMD DDR5: board_name='{}' matched={}",
        board_name,
        board.map(|b| b.model).unwrap_or("none")
    );
    board
}

/// Match a DMI board name against the direct-probe whitelist.
pub fn lookup_board(board_name: &str) -> Option<&'static AmdDdr5Board> {
    let lower = board_name.to_ascii_lowercase();
    SUPPORTED_BOARDS.iter().find(|board| {
        board
            .match_substrings
            .iter()
            .all(|needle| lower.contains(needle))
    })
}

/// Filter the supported board's DesignWare whitelist against discovered buses.
pub fn designware_bus_nums(board: &AmdDdr5Board, buses: &[I2cBus]) -> Vec<u32> {
    let mut result: Vec<u32> = buses
        .iter()
        .filter(|bus| bus.adapter_type == I2cAdapterType::DesignWare)
        .filter(|bus| board.designware_buses.contains(&bus.bus_num))
        .map(|bus| bus.bus_num)
        .collect();
    result.sort_unstable();
    log::debug!(
        "AMD DDR5: board='{}' candidate DesignWare buses={:?}",
        board.model,
        result
    );
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_asus_wrx90e() {
        let board = lookup_board("Pro WS WRX90E-SAGE SE").unwrap();
        assert_eq!(board.model, "ASUS Pro WS WRX90E-SAGE SE");
        assert_eq!(board.designware_buses, &[1, 2]);
    }

    #[test]
    fn matches_asrock_wrx90() {
        let board = lookup_board("WRX90 WS EVO").unwrap();
        assert_eq!(board.model, "ASRock WRX90 WS EVO");
        assert_eq!(board.designware_buses, &[1, 2]);
    }

    #[test]
    fn matches_asus_trx50() {
        let board = lookup_board("Pro WS TRX50-SAGE WIFI A").unwrap();
        assert_eq!(board.model, "ASUS Pro WS TRX50-SAGE WIFI A");
        assert_eq!(board.designware_buses, &[0, 1]);
    }

    #[test]
    fn rejects_unknown_board() {
        assert!(lookup_board("X670E Taichi").is_none());
    }

    #[test]
    fn keeps_only_whitelisted_designware_buses() {
        let buses = vec![
            I2cBus {
                bus_num: 0,
                adapter_type: I2cAdapterType::DesignWare,
            },
            I2cBus {
                bus_num: 1,
                adapter_type: I2cAdapterType::DesignWare,
            },
            I2cBus {
                bus_num: 2,
                adapter_type: I2cAdapterType::DesignWare,
            },
            I2cBus {
                bus_num: 14,
                adapter_type: I2cAdapterType::Piix4Smbus,
            },
        ];

        let board = lookup_board("Pro WS TRX50-SAGE WIFI A").unwrap();
        assert_eq!(designware_bus_nums(board, &buses), vec![0, 1]);
    }
}
