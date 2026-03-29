//! DDR5 I2C bus utilities for direct SPD/temperature probing.

use crate::db::boards::Ddr5BusConfig;
use crate::sensors::i2c::bus_scan::I2cBus;

/// Filter discovered I2C buses to only those whitelisted in the board config.
pub fn filter_buses(config: &Ddr5BusConfig, buses: &[I2cBus]) -> Vec<u32> {
    let mut result: Vec<u32> = buses
        .iter()
        .filter(|bus| config.i2c_buses.contains(&bus.bus_num))
        .map(|bus| bus.bus_num)
        .collect();
    result.sort_unstable();
    log::debug!("DDR5: candidate buses={:?}", result);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sensors::i2c::bus_scan::I2cAdapterType;

    #[test]
    fn keeps_only_whitelisted_i2c_buses() {
        let config = Ddr5BusConfig {
            i2c_buses: &[0, 1],
            slots_per_bus: 4,
        };
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
        assert_eq!(filter_buses(&config, &buses), vec![0, 1]);
    }
}
