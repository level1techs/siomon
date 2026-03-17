//! AMD HSMP telemetry on Windows via SMN mailbox (WinRing0 PCI config).
//!
//! On Linux, HSMP uses the `/dev/hsmp` kernel driver.  On Windows, the same
//! SMN (System Management Network) mailbox is accessible through PCI config
//! space: writing the SMN address to register 0x60 (SMN_INDEX) on the host
//! bridge (Bus 0, Dev 0, Fn 0) and then reading/writing data via register
//! 0x64 (SMN_DATA).
//!
//! The HSMP mailbox registers live at fixed SMN addresses and follow a simple
//! command/response protocol:
//!   1. Write arguments to MSG_ARG registers
//!   2. Clear MSG_RESP to 0
//!   3. Write the command ID to MSG_ID
//!   4. Poll MSG_RESP until it reads 1 (success)
//!   5. Read response arguments from MSG_ARG registers

use crate::model::sensor::{SensorCategory, SensorId, SensorReading, SensorUnit};
use crate::platform::winring0::WinRing0;

// ── SMN access via PCI config registers ─────────────────────────────────────

/// PCI config register on Bus 0 / Dev 0 / Fn 0 used to set the SMN address.
const SMN_INDEX_REG: u8 = 0x60;

/// PCI config register on Bus 0 / Dev 0 / Fn 0 used to read/write SMN data.
const SMN_DATA_REG: u8 = 0x64;

// ── HSMP mailbox SMN addresses ──────────────────────────────────────────────

/// SMN address of the HSMP message-ID register (write command here).
const HSMP_MSG_ID_ADDR: u32 = 0x3B1_0534;

/// SMN address of the HSMP response register (poll until == 1).
const HSMP_MSG_RESP_ADDR: u32 = 0x3B1_0538;

/// SMN base address of the 8 HSMP argument registers (each 4 bytes apart).
const HSMP_MSG_ARG_BASE: u32 = 0x3B1_0998;

// ── HSMP command IDs (same as Linux kernel driver) ──────────────────────────

const HSMP_TEST: u32 = 0x01;
const HSMP_GET_SOCKET_POWER: u32 = 0x04;
const HSMP_GET_SOCKET_POWER_LIMIT: u32 = 0x06;
const HSMP_GET_FCLK_MCLK: u32 = 0x0F;
const HSMP_GET_CCLK_THROTTLE_LIMIT: u32 = 0x10;
const HSMP_GET_C0_PERCENT: u32 = 0x11;
const HSMP_GET_DDR_BANDWIDTH: u32 = 0x14;
const HSMP_GET_RAILS_SVI: u32 = 0x1B;
const HSMP_GET_SOCKET_FMAX_FMIN: u32 = 0x1C;

// ── SMN read/write helpers ──────────────────────────────────────────────────

fn smn_read(wr: &WinRing0, addr: u32) -> u32 {
    wr.write_pci_config_dword(0, 0, 0, SMN_INDEX_REG, addr);
    wr.read_pci_config_dword(0, 0, 0, SMN_DATA_REG)
}

fn smn_write(wr: &WinRing0, addr: u32, val: u32) {
    wr.write_pci_config_dword(0, 0, 0, SMN_INDEX_REG, addr);
    wr.write_pci_config_dword(0, 0, 0, SMN_DATA_REG, val);
}

// ── HSMP mailbox protocol ───────────────────────────────────────────────────

/// Send an HSMP command and return the response arguments.
///
/// `num_args_in` arguments are written from `args_in`, and `num_args_out`
/// response words are read back.  Returns `None` on timeout (mailbox did not
/// respond within ~100ms).
fn hsmp_send(wr: &WinRing0, msg_id: u32, args_in: &[u32], num_args_out: usize) -> Option<Vec<u32>> {
    // 1. Write input arguments
    for (i, &arg) in args_in.iter().enumerate() {
        smn_write(wr, HSMP_MSG_ARG_BASE + (i as u32) * 4, arg);
    }

    // 2. Clear response register
    smn_write(wr, HSMP_MSG_RESP_ADDR, 0);

    // 3. Write command ID to trigger the SMU
    smn_write(wr, HSMP_MSG_ID_ADDR, msg_id);

    // 4. Poll for response (up to ~100ms: 1000 iterations x 100us)
    for _ in 0..1000 {
        let resp = smn_read(wr, HSMP_MSG_RESP_ADDR);
        if resp == 1 {
            // 5. Read response arguments
            let mut results = Vec::with_capacity(num_args_out);
            for i in 0..num_args_out {
                results.push(smn_read(wr, HSMP_MSG_ARG_BASE + (i as u32) * 4));
            }
            return Some(results);
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
    }

    None // timeout
}

// ── Sensor source ───────────────────────────────────────────────────────────

pub struct HsmpWinSource {
    available: bool,
}

impl HsmpWinSource {
    /// Probe for AMD HSMP via WinRing0 SMN mailbox access.
    ///
    /// Validates that:
    ///   1. WinRing0 is loaded
    ///   2. PCI vendor at 0:0:0 is AMD (0x1022)
    ///   3. The HSMP test command (0x01) returns arg0 + 1
    pub fn discover() -> Self {
        let wr = match WinRing0::try_load() {
            Some(w) => w,
            None => {
                log::debug!("HSMP: WinRing0 not available");
                return Self { available: false };
            }
        };

        // Verify AMD CPU by checking PCI vendor ID at Bus 0 / Dev 0 / Fn 0.
        let vendor = wr.read_pci_config_dword(0, 0, 0, 0) & 0xFFFF;
        if vendor != 0x1022 {
            log::debug!("HSMP: not AMD CPU (vendor {:#x})", vendor);
            return Self { available: false };
        }

        // HSMP test: write a known value, expect response == value + 1.
        let test_val: u32 = 0xDEAD;
        if let Some(resp) = hsmp_send(wr, HSMP_TEST, &[test_val], 1) {
            if resp.first() == Some(&(test_val + 1)) {
                log::info!("HSMP: interface validated on AMD CPU via SMN mailbox");
                return Self { available: true };
            }
            log::debug!(
                "HSMP: test response mismatch (expected {:#x}, got {:#x})",
                test_val + 1,
                resp.first().copied().unwrap_or(0)
            );
        } else {
            log::debug!("HSMP: test command timed out");
        }

        Self { available: false }
    }

    pub fn is_available(&self) -> bool {
        self.available
    }
}

impl crate::sensors::SensorSource for HsmpWinSource {
    fn name(&self) -> &str {
        "hsmp"
    }

    fn poll(&mut self) -> Vec<(SensorId, SensorReading)> {
        if !self.available {
            return vec![];
        }

        let wr = match WinRing0::try_load() {
            Some(w) => w,
            None => return vec![],
        };

        let mut readings = Vec::new();

        // Socket power (mW -> W)
        if let Some(resp) = hsmp_send(wr, HSMP_GET_SOCKET_POWER, &[0], 1) {
            let watts = resp[0] as f64 / 1000.0;
            readings.push(sensor(
                "socket_power",
                "Socket Power",
                watts,
                SensorUnit::Watts,
                SensorCategory::Power,
            ));
        }

        // Socket power limit (mW -> W)
        if let Some(resp) = hsmp_send(wr, HSMP_GET_SOCKET_POWER_LIMIT, &[0], 1) {
            let watts = resp[0] as f64 / 1000.0;
            readings.push(sensor(
                "socket_power_limit",
                "Socket Power Limit",
                watts,
                SensorUnit::Watts,
                SensorCategory::Power,
            ));
        }

        // SVI rails power (mW -> W)
        if let Some(resp) = hsmp_send(wr, HSMP_GET_RAILS_SVI, &[0], 1) {
            let watts = resp[0] as f64 / 1000.0;
            readings.push(sensor(
                "svi_power",
                "SVI Rails Power",
                watts,
                SensorUnit::Watts,
                SensorCategory::Power,
            ));
        }

        // FCLK / MCLK (returned as two separate response args)
        if let Some(resp) = hsmp_send(wr, HSMP_GET_FCLK_MCLK, &[0], 2) {
            readings.push(sensor(
                "fclk",
                "Fabric Clock",
                resp[0] as f64,
                SensorUnit::Mhz,
                SensorCategory::Frequency,
            ));
            readings.push(sensor(
                "mclk",
                "Memory Clock",
                resp[1] as f64,
                SensorUnit::Mhz,
                SensorCategory::Frequency,
            ));
        }

        // CCLK throttle limit
        if let Some(resp) = hsmp_send(wr, HSMP_GET_CCLK_THROTTLE_LIMIT, &[0], 1) {
            readings.push(sensor(
                "cclk_limit",
                "CCLK Throttle Limit",
                resp[0] as f64,
                SensorUnit::Mhz,
                SensorCategory::Frequency,
            ));
        }

        // C0 residency (%)
        if let Some(resp) = hsmp_send(wr, HSMP_GET_C0_PERCENT, &[0], 1) {
            readings.push(sensor(
                "c0_residency",
                "C0 Residency",
                resp[0] as f64,
                SensorUnit::Percent,
                SensorCategory::Utilization,
            ));
        }

        // DDR bandwidth: max[31:20] used[19:8] pct[7:0]
        if let Some(resp) = hsmp_send(wr, HSMP_GET_DDR_BANDWIDTH, &[0], 1) {
            let raw = resp[0];
            let max_gbps = ((raw >> 20) & 0xFFF) as f64;
            let used_gbps = ((raw >> 8) & 0xFFF) as f64;
            let pct = (raw & 0xFF) as f64;
            readings.push(sensor(
                "ddr_bw_max",
                "DDR BW Max",
                max_gbps,
                SensorUnit::Unitless,
                SensorCategory::Throughput,
            ));
            readings.push(sensor(
                "ddr_bw_used",
                "DDR BW Used",
                used_gbps,
                SensorUnit::Unitless,
                SensorCategory::Throughput,
            ));
            readings.push(sensor(
                "ddr_bw_util",
                "DDR BW Utilization",
                pct,
                SensorUnit::Percent,
                SensorCategory::Utilization,
            ));
        }

        // Fmax / Fmin: fmax[31:16] fmin[15:0]
        if let Some(resp) = hsmp_send(wr, HSMP_GET_SOCKET_FMAX_FMIN, &[0], 1) {
            let raw = resp[0];
            let fmax = ((raw >> 16) & 0xFFFF) as f64;
            let fmin = (raw & 0xFFFF) as f64;
            readings.push(sensor(
                "fmax",
                "Socket Fmax",
                fmax,
                SensorUnit::Mhz,
                SensorCategory::Frequency,
            ));
            readings.push(sensor(
                "fmin",
                "Socket Fmin",
                fmin,
                SensorUnit::Mhz,
                SensorCategory::Frequency,
            ));
        }

        readings
    }
}

// ── Helper ──────────────────────────────────────────────────────────────────

fn sensor(
    name: &str,
    label: &str,
    value: f64,
    unit: SensorUnit,
    category: SensorCategory,
) -> (SensorId, SensorReading) {
    let id = SensorId {
        source: "hsmp".into(),
        chip: "smu".into(),
        sensor: name.into(),
    };
    (
        id,
        SensorReading::new(label.to_string(), value, unit, category),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_without_hardware() {
        // Without WinRing0 loaded, discover should gracefully return unavailable.
        let src = HsmpWinSource::discover();
        assert!(!src.is_available());
    }

    #[test]
    fn sensor_helper_produces_correct_ids() {
        let (id, reading) = sensor(
            "socket_power",
            "Socket Power",
            142.5,
            SensorUnit::Watts,
            SensorCategory::Power,
        );
        assert_eq!(id.source, "hsmp");
        assert_eq!(id.chip, "smu");
        assert_eq!(id.sensor, "socket_power");
        assert_eq!(reading.label, "Socket Power");
        assert_eq!(reading.current, 142.5);
        assert_eq!(reading.unit, SensorUnit::Watts);
        assert_eq!(reading.category, SensorCategory::Power);
    }
}
