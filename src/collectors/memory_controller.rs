//! Trained memory-controller configuration from Intel MCHBAR (`--direct-io`).
//!
//! Decodes the DDR5 data rate, gear, command rate and the full trained
//! timing set from the first channel whose timings are plausible. Read once
//! at startup: these registers are written by the BIOS during training and
//! do not change at runtime.

use crate::model::memory::{MemoryControllerInfo, TrainedTimings};
use crate::platform::mchbar::{self, Mchbar};

/// Extract the inclusive bit field `hi..=lo` of a register word.
fn bits(value: u64, hi: u32, lo: u32) -> u64 {
    (value >> lo) & ((1u64 << (hi - lo + 1)) - 1)
}

/// Raw timing-register words of one channel.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct TimingWords {
    pub pre: u64,
    pub rdrd: u32,
    pub rdwr: u32,
    pub wrrd: u32,
    pub wrwr: u32,
    pub cas: u32,
    pub gs_cfg: u64,
    pub act: u64,
    pub rftp: u64,
}

pub(crate) fn decode_timings(w: &TimingWords) -> TrainedTimings {
    let (pre, act, rftp) = (w.pre, w.act, w.rftp);
    let field = |word: u32, hi: u32, lo: u32| bits(word as u64, hi, lo) as u32;
    let field64 = |word: u64, hi: u32, lo: u32| bits(word, hi, lo) as u32;

    let trp = field64(pre, 7, 0);
    let tras = field64(pre, 53, 45);
    let twtp = field64(pre, 42, 33);
    let tcwl = field(w.cas, 31, 24);
    TrainedTimings {
        tcl: field(w.cas, 22, 16),
        trcd: field64(act, 29, 22),
        trcdw: field64(act, 39, 32),
        trp,
        trpab: field64(pre, 17, 10),
        tras,
        trc: tras + trp,
        tcwl,
        // tWRPRE = tCWL + BL16/2 + tWR
        twr: twtp.saturating_sub(tcwl + 8),
        twtp,
        trtp: field64(pre, 26, 20),
        tfaw: field64(act, 8, 0),
        trrd_l: field64(act, 14, 9),
        trrd_s: field64(act, 21, 15),
        trfc: field64(rftp, 30, 18),
        trfcpb: field64(rftp, 50, 40),
        trefi: field64(rftp, 17, 0),
        trefsbrd: field64(rftp, 58, 51),
        trdrd: [
            field(w.rdrd, 6, 0),
            field(w.rdrd, 14, 8),
            field(w.rdrd, 23, 16),
            field(w.rdrd, 31, 24),
        ],
        trdwr: [
            field(w.rdwr, 7, 0),
            field(w.rdwr, 15, 8),
            field(w.rdwr, 23, 16),
            field(w.rdwr, 31, 24),
        ],
        twrrd: [
            field(w.wrrd, 8, 0),
            field(w.wrrd, 17, 9),
            field(w.wrrd, 24, 18),
            field(w.wrrd, 31, 25),
        ],
        twrwr: [
            field(w.wrwr, 6, 0),
            field(w.wrwr, 14, 8),
            field(w.wrwr, 22, 16),
            field(w.wrwr, 31, 24),
        ],
    }
}

/// Reject channels whose primaries are outside DDR5 ranges (unpopulated or
/// unreadable channels read as zero or all-ones).
pub(crate) fn plausible(t: &TrainedTimings) -> bool {
    (4..=72).contains(&t.tcl)
        && (8..=59).contains(&t.trcd)
        && (10..=76).contains(&t.trp)
        && (28..=136).contains(&t.tras)
        && (16..=88).contains(&t.tfaw)
        && t.trfc > 0
}

/// `(gear, command rate)` from `SC_GS_CFG`: bit 31 selects Gear 4 over
/// Gear 2, bit 3 selects 2N over 1N.
pub(crate) fn gear_and_command_rate(gs_cfg: u64) -> (u8, u8) {
    let gear = if bits(gs_cfg, 31, 31) == 1 { 4 } else { 2 };
    let cmd = if bits(gs_cfg, 3, 3) == 1 { 2 } else { 1 };
    (gear, cmd)
}

/// QCLK in MHz from `MEMSS_PMA_CR_BIOS_DATA[7:0]` (33.33 MHz reference).
pub(crate) fn qclk_mhz(bios_data: u32) -> Option<f64> {
    let ratio = bios_data & 0xFF;
    (bios_data != u32::MAX && ratio != 0).then(|| ratio as f64 * 100.0 / 3.0)
}

/// Theoretical bandwidth ceiling in MB/s from `BIOS_REQ[28:9]`.
pub(crate) fn max_bandwidth_mbs(bios_req: u32) -> Option<u32> {
    let mbs = (bios_req >> 9) & 0xF_FFFF;
    (bios_req != u32::MAX && mbs != 0).then_some(mbs)
}

fn read_words(m: &Mchbar, base: u64) -> Option<TimingWords> {
    Some(TimingWords {
        pre: m.read64(base + mchbar::TC_PRE)?,
        rdrd: m.read32(base + mchbar::TC_RDRD)?,
        rdwr: m.read32(base + mchbar::TC_RDWR)?,
        wrrd: m.read32(base + mchbar::TC_WRRD)?,
        wrwr: m.read32(base + mchbar::TC_WRWR)?,
        cas: m.read32(base + mchbar::TC_CAS)?,
        gs_cfg: m.read64(base + mchbar::SC_GS_CFG)?,
        act: m.read64(base + mchbar::TC_ACT)?,
        rftp: m.read64(base + mchbar::TC_RFTP)?,
    })
}

/// Read the trained configuration, or `None` if MCHBAR is unavailable or no
/// channel has plausible timings.
pub fn collect() -> Option<MemoryControllerInfo> {
    let m = Mchbar::open()?;
    let (source, words, timings) = mchbar::TIMING_INSTANCES.iter().find_map(|&(base, name)| {
        let words = read_words(&m, base)?;
        let timings = decode_timings(&words);
        plausible(&timings).then_some((name, words, timings))
    })?;
    let (gear, command_rate) = gear_and_command_rate(words.gs_cfg);
    let qclk = m.read32(mchbar::MEMSS_BIOS_DATA).and_then(qclk_mhz);

    Some(MemoryControllerInfo {
        source: format!("{} MCHBAR, {source}", m.platform()),
        qclk_mhz: qclk,
        data_rate_mts: qclk.map(|mhz| (mhz * 2.0).round() as u32),
        gear,
        command_rate,
        max_bandwidth_mbs: m.read32(mchbar::MEMSS_BIOS_REQ).and_then(max_bandwidth_mbs),
        timings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Channel 0 words recorded on a Z890 Steel Legend WiFi, BIOS 3.40,
    /// DDR5-7200 Gear 2.
    fn recorded() -> TimingWords {
        TimingWords {
            pre: 0x000C_0134_2170_B02C,
            rdrd: 0x1010_0812,
            rdwr: 0x1515_1515,
            wrrd: 0x1E3C_6446,
            wrwr: 0x1212_0848,
            cas: 0x2424_0024,
            gs_cfg: 0x0010_8002_0000_0009,
            act: 0x0000_002C_0B04_1820,
            rftp: 0x0369_D3FF_08F8_7FFF,
        }
    }

    #[test]
    fn test_primary_timings() {
        let t = decode_timings(&recorded());
        assert_eq!((t.tcl, t.trcd, t.trp, t.tras), (36, 44, 44, 96));
        assert_eq!(t.trc, 140);
        assert!(plausible(&t));
    }

    #[test]
    fn test_secondary_timings() {
        let t = decode_timings(&recorded());
        assert_eq!((t.tcwl, t.twr, t.twtp), (36, 110, 154));
        assert_eq!((t.trtp, t.trrd_l, t.trrd_s, t.tfaw), (23, 12, 8, 32));
        assert_eq!((t.trcdw, t.trpab), (44, 44));
        assert_eq!(
            (t.trfc, t.trfcpb, t.trefi, t.trefsbrd),
            (574, 467, 32767, 109)
        );
    }

    #[test]
    fn test_turnarounds() {
        let t = decode_timings(&recorded());
        assert_eq!(t.trdrd, [18, 8, 16, 16]);
        assert_eq!(t.trdwr, [21, 21, 21, 21]);
        assert_eq!(t.twrrd, [70, 50, 15, 15]);
        assert_eq!(t.twrwr, [72, 8, 18, 18]);
    }

    #[test]
    fn test_unpopulated_channel_is_implausible() {
        assert!(!plausible(&decode_timings(&TimingWords::default())));
        let ones = TimingWords {
            pre: u64::MAX,
            rdrd: u32::MAX,
            rdwr: u32::MAX,
            wrrd: u32::MAX,
            wrwr: u32::MAX,
            cas: u32::MAX,
            gs_cfg: u64::MAX,
            act: u64::MAX,
            rftp: u64::MAX,
        };
        assert!(!plausible(&decode_timings(&ones)));
    }

    #[test]
    fn test_gear_cmd_qclk_bandwidth() {
        assert_eq!(gear_and_command_rate(0x0010_8002_0000_0009), (2, 2));
        assert_eq!(gear_and_command_rate(0x8000_0000), (4, 1));
        assert_eq!(qclk_mhz(0x6C), Some(3600.0));
        assert_eq!(qclk_mhz(0), None);
        assert_eq!(max_bandwidth_mbs(0x0384_006C), Some(115_200));
        assert_eq!(max_bandwidth_mbs(u32::MAX), None);
    }
}
