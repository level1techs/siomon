//! Safe wrapper around the NVIDIA Management Library (NVML).
//!
//! Uses `libloading` to dynamically load `libnvidia-ml.so.1` at runtime,
//! so the binary runs on systems without NVIDIA drivers installed.

#![cfg(feature = "nvidia")]

use std::ffi::{CStr, c_char, c_uint};
use std::sync::Arc;

use crate::error::NvmlError;

// ---------------------------------------------------------------------------
// NVML constants
// ---------------------------------------------------------------------------

const NVML_SUCCESS: c_uint = 0;

/// Clock type constants for `nvmlDeviceGetClockInfo` / `nvmlDeviceGetMaxClockInfo`.
pub const NVML_CLOCK_GRAPHICS: c_uint = 0;
pub const NVML_CLOCK_MEM: c_uint = 2;

/// Temperature sensor constants.
const NVML_TEMPERATURE_GPU: c_uint = 0;

// ---------------------------------------------------------------------------
// C-compatible structs returned by NVML functions
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct NvmlUtilization {
    pub gpu: u32,
    pub memory: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct NvmlMemoryInfo {
    pub total: u64,
    pub free: u64,
    pub used: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct NvmlPciInfo {
    pub bus_id_legacy: [c_char; 16],
    pub domain: u32,
    pub bus: u32,
    pub device: u32,
    pub pci_device_id: u32,
    pub pci_subsystem_id: u32,
    pub bus_id: [c_char; 32],
}

// ---------------------------------------------------------------------------
// Opaque device handle (pointer-sized)
// ---------------------------------------------------------------------------

type NvmlDevice = *mut std::ffi::c_void;

// ---------------------------------------------------------------------------
// Function pointer type aliases
// ---------------------------------------------------------------------------

type FnInit = unsafe extern "C" fn() -> c_uint;
type FnShutdown = unsafe extern "C" fn() -> c_uint;
type FnDeviceGetCount = unsafe extern "C" fn(*mut c_uint) -> c_uint;
type FnDeviceGetHandleByIndex = unsafe extern "C" fn(c_uint, *mut NvmlDevice) -> c_uint;
type FnDeviceGetName = unsafe extern "C" fn(NvmlDevice, *mut c_char, c_uint) -> c_uint;
type FnDeviceGetPciInfo = unsafe extern "C" fn(NvmlDevice, *mut NvmlPciInfo) -> c_uint;
type FnDeviceGetTemperature = unsafe extern "C" fn(NvmlDevice, c_uint, *mut c_uint) -> c_uint;
type FnDeviceGetFanSpeed = unsafe extern "C" fn(NvmlDevice, *mut c_uint) -> c_uint;
type FnDeviceGetPowerUsage = unsafe extern "C" fn(NvmlDevice, *mut c_uint) -> c_uint;
type FnDeviceGetClockInfo = unsafe extern "C" fn(NvmlDevice, c_uint, *mut c_uint) -> c_uint;
type FnDeviceGetMaxClockInfo = unsafe extern "C" fn(NvmlDevice, c_uint, *mut c_uint) -> c_uint;
type FnDeviceGetUtilizationRates = unsafe extern "C" fn(NvmlDevice, *mut NvmlUtilization) -> c_uint;
type FnDeviceGetMemoryInfo = unsafe extern "C" fn(NvmlDevice, *mut NvmlMemoryInfo) -> c_uint;
type FnDeviceGetPowerManagementLimit = unsafe extern "C" fn(NvmlDevice, *mut c_uint) -> c_uint;
type FnDeviceGetVbiosVersion = unsafe extern "C" fn(NvmlDevice, *mut c_char, c_uint) -> c_uint;
type FnSystemGetDriverVersion = unsafe extern "C" fn(*mut c_char, c_uint) -> c_uint;
type FnDeviceGetCurrPcieLinkGeneration = unsafe extern "C" fn(NvmlDevice, *mut c_uint) -> c_uint;
type FnDeviceGetCurrPcieLinkWidth = unsafe extern "C" fn(NvmlDevice, *mut c_uint) -> c_uint;

// ---------------------------------------------------------------------------
// NvmlLibrary — the main handle
// ---------------------------------------------------------------------------

/// Dynamic wrapper around `libnvidia-ml.so.1`.
///
/// Holds the `libloading::Library` (keeping the .so mapped) and resolved
/// function pointers for every NVML call we use.
pub struct NvmlLibrary {
    // Arc so that NvmlLibrary is Send (libloading::Library is not Send by
    // default, but the NVML library is thread-safe once initialised).
    _lib: Arc<libloading::Library>,

    // Resolved function pointers
    fn_shutdown: FnShutdown,
    fn_device_get_count: FnDeviceGetCount,
    fn_device_get_handle_by_index: FnDeviceGetHandleByIndex,
    fn_device_get_name: FnDeviceGetName,
    fn_device_get_pci_info: FnDeviceGetPciInfo,
    fn_device_get_temperature: FnDeviceGetTemperature,
    fn_device_get_fan_speed: FnDeviceGetFanSpeed,
    fn_device_get_power_usage: FnDeviceGetPowerUsage,
    fn_device_get_clock_info: FnDeviceGetClockInfo,
    fn_device_get_max_clock_info: FnDeviceGetMaxClockInfo,
    fn_device_get_utilization_rates: FnDeviceGetUtilizationRates,
    fn_device_get_memory_info: FnDeviceGetMemoryInfo,
    fn_device_get_power_management_limit: FnDeviceGetPowerManagementLimit,
    fn_device_get_vbios_version: FnDeviceGetVbiosVersion,
    fn_system_get_driver_version: FnSystemGetDriverVersion,
    fn_device_get_curr_pcie_link_generation: FnDeviceGetCurrPcieLinkGeneration,
    fn_device_get_curr_pcie_link_width: FnDeviceGetCurrPcieLinkWidth,
}

impl NvmlLibrary {
    /// Try to load the NVML shared library and initialise it.
    ///
    /// Returns `None` if the library cannot be found (driver not installed)
    /// or if `nvmlInit_v2` fails.
    pub fn try_load() -> Option<Self> {
        // SAFETY: We are loading a well-known system library path.
        #[cfg(unix)]
        let lib = unsafe { libloading::Library::new("libnvidia-ml.so.1") }.ok()?;
        #[cfg(windows)]
        let lib = unsafe { libloading::Library::new("nvml.dll") }.ok()?;

        // Resolve all symbols. If any mandatory symbol is missing we bail out.
        unsafe {
            let fn_init: FnInit = *lib.get(b"nvmlInit_v2\0").ok()?;
            let fn_shutdown: FnShutdown = *lib.get(b"nvmlShutdown\0").ok()?;
            let fn_device_get_count: FnDeviceGetCount =
                *lib.get(b"nvmlDeviceGetCount_v2\0").ok()?;
            let fn_device_get_handle_by_index: FnDeviceGetHandleByIndex =
                *lib.get(b"nvmlDeviceGetHandleByIndex_v2\0").ok()?;
            let fn_device_get_name: FnDeviceGetName = *lib.get(b"nvmlDeviceGetName\0").ok()?;
            let fn_device_get_pci_info: FnDeviceGetPciInfo =
                *lib.get(b"nvmlDeviceGetPciInfo_v3\0").ok()?;
            let fn_device_get_temperature: FnDeviceGetTemperature =
                *lib.get(b"nvmlDeviceGetTemperature\0").ok()?;
            let fn_device_get_fan_speed: FnDeviceGetFanSpeed =
                *lib.get(b"nvmlDeviceGetFanSpeed\0").ok()?;
            let fn_device_get_power_usage: FnDeviceGetPowerUsage =
                *lib.get(b"nvmlDeviceGetPowerUsage\0").ok()?;
            let fn_device_get_clock_info: FnDeviceGetClockInfo =
                *lib.get(b"nvmlDeviceGetClockInfo\0").ok()?;
            let fn_device_get_max_clock_info: FnDeviceGetMaxClockInfo =
                *lib.get(b"nvmlDeviceGetMaxClockInfo\0").ok()?;
            let fn_device_get_utilization_rates: FnDeviceGetUtilizationRates =
                *lib.get(b"nvmlDeviceGetUtilizationRates\0").ok()?;
            let fn_device_get_memory_info: FnDeviceGetMemoryInfo =
                *lib.get(b"nvmlDeviceGetMemoryInfo\0").ok()?;
            let fn_device_get_power_management_limit: FnDeviceGetPowerManagementLimit =
                *lib.get(b"nvmlDeviceGetPowerManagementLimit\0").ok()?;
            let fn_device_get_vbios_version: FnDeviceGetVbiosVersion =
                *lib.get(b"nvmlDeviceGetVbiosVersion\0").ok()?;
            let fn_system_get_driver_version: FnSystemGetDriverVersion =
                *lib.get(b"nvmlSystemGetDriverVersion\0").ok()?;
            let fn_device_get_curr_pcie_link_generation: FnDeviceGetCurrPcieLinkGeneration =
                *lib.get(b"nvmlDeviceGetCurrPcieLinkGeneration\0").ok()?;
            let fn_device_get_curr_pcie_link_width: FnDeviceGetCurrPcieLinkWidth =
                *lib.get(b"nvmlDeviceGetCurrPcieLinkWidth\0").ok()?;

            // Initialise the library.
            let ret = fn_init();
            if ret != NVML_SUCCESS {
                log::warn!("nvmlInit_v2 failed with error code {ret}");
                return None;
            }

            Some(Self {
                _lib: Arc::new(lib),
                fn_shutdown,
                fn_device_get_count,
                fn_device_get_handle_by_index,
                fn_device_get_name,
                fn_device_get_pci_info,
                fn_device_get_temperature,
                fn_device_get_fan_speed,
                fn_device_get_power_usage,
                fn_device_get_clock_info,
                fn_device_get_max_clock_info,
                fn_device_get_utilization_rates,
                fn_device_get_memory_info,
                fn_device_get_power_management_limit,
                fn_device_get_vbios_version,
                fn_system_get_driver_version,
                fn_device_get_curr_pcie_link_generation,
                fn_device_get_curr_pcie_link_width,
            })
        }
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    fn get_handle(&self, index: u32) -> Result<NvmlDevice, NvmlError> {
        let mut handle: NvmlDevice = std::ptr::null_mut();
        let ret = unsafe { (self.fn_device_get_handle_by_index)(index as c_uint, &mut handle) };
        nvml_check(ret)?;
        Ok(handle)
    }

    fn read_c_string(buf: &[c_char]) -> String {
        // SAFETY: buffer is null-terminated by NVML.
        unsafe { CStr::from_ptr(buf.as_ptr()) }
            .to_string_lossy()
            .into_owned()
    }

    // -----------------------------------------------------------------------
    // Public safe wrappers
    // -----------------------------------------------------------------------

    /// Number of NVIDIA GPUs visible to NVML.
    pub fn device_count(&self) -> Result<u32, NvmlError> {
        let mut count: c_uint = 0;
        let ret = unsafe { (self.fn_device_get_count)(&mut count) };
        nvml_check(ret)?;
        Ok(count)
    }

    /// Product name (e.g. "NVIDIA GeForce RTX 4090").
    pub fn device_name(&self, index: u32) -> Result<String, NvmlError> {
        let handle = self.get_handle(index)?;
        let mut buf = [0 as c_char; 96];
        let ret =
            unsafe { (self.fn_device_get_name)(handle, buf.as_mut_ptr(), buf.len() as c_uint) };
        nvml_check(ret)?;
        Ok(Self::read_c_string(&buf))
    }

    /// GPU core temperature in degrees Celsius.
    pub fn device_temperature(&self, index: u32) -> Result<u32, NvmlError> {
        let handle = self.get_handle(index)?;
        let mut temp: c_uint = 0;
        let ret =
            unsafe { (self.fn_device_get_temperature)(handle, NVML_TEMPERATURE_GPU, &mut temp) };
        nvml_check(ret)?;
        Ok(temp)
    }

    /// Fan speed as a percentage (0-100).
    pub fn device_fan_speed(&self, index: u32) -> Result<u32, NvmlError> {
        let handle = self.get_handle(index)?;
        let mut speed: c_uint = 0;
        let ret = unsafe { (self.fn_device_get_fan_speed)(handle, &mut speed) };
        nvml_check(ret)?;
        Ok(speed)
    }

    /// Current power draw in watts.
    ///
    /// NVML reports milliwatts; this method converts to watts.
    pub fn device_power_watts(&self, index: u32) -> Result<f64, NvmlError> {
        let handle = self.get_handle(index)?;
        let mut mw: c_uint = 0;
        let ret = unsafe { (self.fn_device_get_power_usage)(handle, &mut mw) };
        nvml_check(ret)?;
        Ok(mw as f64 / 1000.0)
    }

    /// Current clock speed in MHz for the given clock domain.
    pub fn device_clock_mhz(&self, index: u32, clock_type: u32) -> Result<u32, NvmlError> {
        let handle = self.get_handle(index)?;
        let mut mhz: c_uint = 0;
        let ret =
            unsafe { (self.fn_device_get_clock_info)(handle, clock_type as c_uint, &mut mhz) };
        nvml_check(ret)?;
        Ok(mhz)
    }

    /// Maximum clock speed in MHz for the given clock domain.
    pub fn device_max_clock_mhz(&self, index: u32, clock_type: u32) -> Result<u32, NvmlError> {
        let handle = self.get_handle(index)?;
        let mut mhz: c_uint = 0;
        let ret =
            unsafe { (self.fn_device_get_max_clock_info)(handle, clock_type as c_uint, &mut mhz) };
        nvml_check(ret)?;
        Ok(mhz)
    }

    /// GPU and memory utilisation percentages.
    pub fn device_utilization(&self, index: u32) -> Result<NvmlUtilization, NvmlError> {
        let handle = self.get_handle(index)?;
        let mut util = NvmlUtilization::default();
        let ret = unsafe { (self.fn_device_get_utilization_rates)(handle, &mut util) };
        nvml_check(ret)?;
        Ok(util)
    }

    /// Total, free, and used VRAM in bytes.
    pub fn device_memory_info(&self, index: u32) -> Result<NvmlMemoryInfo, NvmlError> {
        let handle = self.get_handle(index)?;
        let mut info = NvmlMemoryInfo::default();
        let ret = unsafe { (self.fn_device_get_memory_info)(handle, &mut info) };
        nvml_check(ret)?;
        Ok(info)
    }

    /// VBIOS version string.
    pub fn device_vbios_version(&self, index: u32) -> Result<String, NvmlError> {
        let handle = self.get_handle(index)?;
        let mut buf = [0 as c_char; 64];
        let ret = unsafe {
            (self.fn_device_get_vbios_version)(handle, buf.as_mut_ptr(), buf.len() as c_uint)
        };
        nvml_check(ret)?;
        Ok(Self::read_c_string(&buf))
    }

    /// NVIDIA driver version string (e.g. "550.54.14").
    pub fn driver_version(&self) -> Result<String, NvmlError> {
        let mut buf = [0 as c_char; 80];
        let ret =
            unsafe { (self.fn_system_get_driver_version)(buf.as_mut_ptr(), buf.len() as c_uint) };
        nvml_check(ret)?;
        Ok(Self::read_c_string(&buf))
    }

    /// Current PCIe link generation.
    pub fn device_pcie_gen(&self, index: u32) -> Result<u32, NvmlError> {
        let handle = self.get_handle(index)?;
        let mut generation: c_uint = 0;
        let ret =
            unsafe { (self.fn_device_get_curr_pcie_link_generation)(handle, &mut generation) };
        nvml_check(ret)?;
        Ok(generation)
    }

    /// Current PCIe link width (number of lanes).
    pub fn device_pcie_width(&self, index: u32) -> Result<u32, NvmlError> {
        let handle = self.get_handle(index)?;
        let mut width: c_uint = 0;
        let ret = unsafe { (self.fn_device_get_curr_pcie_link_width)(handle, &mut width) };
        nvml_check(ret)?;
        Ok(width)
    }

    /// Board power limit in watts.
    ///
    /// NVML reports milliwatts; this method converts to watts.
    pub fn device_power_limit_watts(&self, index: u32) -> Result<f64, NvmlError> {
        let handle = self.get_handle(index)?;
        let mut mw: c_uint = 0;
        let ret = unsafe { (self.fn_device_get_power_management_limit)(handle, &mut mw) };
        nvml_check(ret)?;
        Ok(mw as f64 / 1000.0)
    }

    /// PCI bus information for the device.
    pub fn device_pci_info(&self, index: u32) -> Result<NvmlPciInfo, NvmlError> {
        let handle = self.get_handle(index)?;
        let mut info = NvmlPciInfo::default();
        let ret = unsafe { (self.fn_device_get_pci_info)(handle, &mut info) };
        nvml_check(ret)?;
        Ok(info)
    }
}

impl Drop for NvmlLibrary {
    fn drop(&mut self) {
        let ret = unsafe { (self.fn_shutdown)() };
        if ret != NVML_SUCCESS {
            log::warn!("nvmlShutdown returned error code {ret}");
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn nvml_check(ret: c_uint) -> Result<(), NvmlError> {
    if ret == NVML_SUCCESS {
        Ok(())
    } else {
        Err(NvmlError::ApiError(ret))
    }
}
