//! MSR (Model-Specific Register) reading support on Windows via WinRing0.

/// Read a 64-bit MSR by index. Returns `None` if WinRing0 is unavailable or the read fails.
pub fn read_msr(index: u32) -> Option<u64> {
    super::winring0::WinRing0::try_load()?.read_msr(index)
}
