#[cfg(all(windows, feature = "nvidia"))]
pub mod adl;
#[cfg(unix)]
pub mod msr;
#[cfg(windows)]
pub mod msr_win;
#[cfg(unix)]
pub mod nvme_ioctl;
#[cfg(windows)]
pub mod nvme_win;
#[cfg(feature = "nvidia")]
pub mod nvml;
#[cfg(unix)]
pub mod port_io;
#[cfg(windows)]
pub mod port_io_win;
pub mod procfs;
#[cfg(unix)]
pub mod sata_ioctl;
#[cfg(windows)]
pub mod sata_win;
#[cfg(unix)]
pub mod sinfo_io;
#[cfg(windows)]
pub mod sinfo_io_win;
#[cfg(windows)]
pub mod smbus_win;
pub mod sysfs;
#[cfg(windows)]
pub mod winring0;

/// Check whether the current process is running with elevated (Administrator)
/// privileges on Windows, using the token elevation API.
///
/// On Unix, returns whether the effective user ID is 0 (root).
#[cfg(windows)]
pub fn is_elevated() -> bool {
    use std::mem;
    use std::ptr;
    use winapi::um::handleapi::CloseHandle;
    use winapi::um::processthreadsapi::{GetCurrentProcess, OpenProcessToken};
    use winapi::um::securitybaseapi::GetTokenInformation;
    use winapi::um::winnt::{TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};

    unsafe {
        let mut token = ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }

        let mut elevation: TOKEN_ELEVATION = mem::zeroed();
        let mut returned: u32 = 0;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            &mut elevation as *mut TOKEN_ELEVATION as *mut _,
            mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        );
        CloseHandle(token);

        ok != 0 && elevation.TokenIsElevated != 0
    }
}

#[cfg(unix)]
pub fn is_elevated() -> bool {
    unsafe { libc::geteuid() == 0 }
}
