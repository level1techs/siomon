//! AMD Display Library (ADL) wrapper for GPU sensor reading on Windows.
//!
//! Uses `libloading` to dynamically load `atiadlxx.dll` at runtime,
//! so the binary runs on systems without AMD drivers installed.
//! The ADL2 API is preferred for its per-context design.

use std::ffi::{CStr, c_char, c_int, c_void};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// ADL constants
// ---------------------------------------------------------------------------

const ADL_OK: c_int = 0;

/// Adapter-active flag for `iExist` / `iPresent`.
const ADL_TRUE: c_int = 1;

/// Overdrive version constants.
#[allow(dead_code)]
const ADL_OD6_TEMPERATURE_EN: c_int = 0x0001; // thermal sensor available
#[allow(dead_code)]
const ADL_OD6_FANSPEED_EN: c_int = 0x0002; // fan speed readable

/// PM-activity reporting — reserved for future Overdrive N / PMLog support.
#[allow(dead_code)]
const ADL_PMLOG_TEMPERATURE_EDGE: c_int = 0;
#[allow(dead_code)]
const ADL_PMLOG_TEMPERATURE_HOTSPOT: c_int = 1;
#[allow(dead_code)]
const ADL_PMLOG_FAN_RPM: c_int = 2;
#[allow(dead_code)]
const ADL_PMLOG_FAN_PERCENTAGE: c_int = 3;
#[allow(dead_code)]
const ADL_PMLOG_CLK_GFXCLK: c_int = 4;
#[allow(dead_code)]
const ADL_PMLOG_CLK_MEMCLK: c_int = 5;

// ---------------------------------------------------------------------------
// ADL memory-allocation callback
// ---------------------------------------------------------------------------

/// ADL requires the caller to provide a malloc-compatible allocator.
/// This is called from inside ADL to allocate buffers for adapter info etc.
extern "system" fn adl_malloc(size: c_int) -> *mut c_void {
    // SAFETY: We delegate to the system allocator via the C runtime.
    unsafe {
        let layout = std::alloc::Layout::from_size_align_unchecked(size as usize, 8);
        std::alloc::alloc(layout) as *mut c_void
    }
}

// ---------------------------------------------------------------------------
// C-compatible structs returned by ADL
// ---------------------------------------------------------------------------

/// Adapter information structure (ADL_Adapter_AdapterInfo_Get).
/// The actual struct has many more fields, but we only keep the ones we need
/// plus padding to get the total size correct (the struct is 256 + several
/// arrays). We use a conservatively sized version.
#[repr(C)]
#[derive(Clone)]
pub struct AdlAdapterInfo {
    pub size: c_int,
    pub adapter_index: c_int,
    pub udid: [c_char; 256],
    pub bus_number: c_int,
    pub device_number: c_int,
    pub function_number: c_int,
    pub vendor_id: c_int,
    pub adapter_name: [c_char; 256],
    pub display_name: [c_char; 256],
    pub present: c_int,
    pub exist: c_int,
    pub driver_path: [c_char; 256],
    pub driver_path_ext: [c_char; 256],
    pub pnp_string: [c_char; 256],
    pub os_display_index: c_int,
}

impl Default for AdlAdapterInfo {
    fn default() -> Self {
        // SAFETY: All-zero is valid for this POD struct.
        unsafe { std::mem::zeroed() }
    }
}

/// Overdrive 6 thermal controller capabilities.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct AdlOD6ThermalControllerCaps {
    pub capabilities: c_int,
    pub fan_min_percent: c_int,
    pub fan_max_percent: c_int,
    pub fan_min_rpm: c_int,
    pub fan_max_rpm: c_int,
    pub temperature_min: c_int,
    pub temperature_max: c_int,
}

/// Overdrive 6 current fan-speed info.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct AdlOD6FanSpeedInfo {
    pub speed_type: c_int,
    pub fan_speed_percent: c_int,
    pub fan_speed_rpm: c_int,
}

// ---------------------------------------------------------------------------
// Function pointer type aliases (ADL2 API)
// ---------------------------------------------------------------------------

/// ADL2_Main_Control_Create(callback, iEnumConnectedAdapters, *context)
type FnAdl2MainControlCreate = unsafe extern "system" fn(
    extern "system" fn(c_int) -> *mut c_void,
    c_int,
    *mut *mut c_void,
) -> c_int;

/// ADL2_Main_Control_Destroy(context)
type FnAdl2MainControlDestroy = unsafe extern "system" fn(*mut c_void) -> c_int;

/// ADL2_Adapter_NumberOfAdapters_Get(context, *count)
type FnAdl2AdapterNumberGet = unsafe extern "system" fn(*mut c_void, *mut c_int) -> c_int;

/// ADL2_Adapter_AdapterInfo_Get(context, *info, size)
type FnAdl2AdapterInfoGet =
    unsafe extern "system" fn(*mut c_void, *mut AdlAdapterInfo, c_int) -> c_int;

/// ADL2_Adapter_Active_Get(context, adapterIndex, *status)
type FnAdl2AdapterActiveGet = unsafe extern "system" fn(*mut c_void, c_int, *mut c_int) -> c_int;

/// ADL2_Overdrive6_Temperature_Get(context, adapterIndex, *temperature)
/// Temperature returned in milli-degrees Celsius.
type FnAdl2OD6TemperatureGet = unsafe extern "system" fn(*mut c_void, c_int, *mut c_int) -> c_int;

/// ADL2_Overdrive6_FanSpeed_Get(context, adapterIndex, *fanSpeedInfo)
type FnAdl2OD6FanSpeedGet =
    unsafe extern "system" fn(*mut c_void, c_int, *mut AdlOD6FanSpeedInfo) -> c_int;

/// ADL2_Overdrive6_ThermalController_Caps(context, adapterIndex, *caps)
type FnAdl2OD6ThermalCaps =
    unsafe extern "system" fn(*mut c_void, c_int, *mut AdlOD6ThermalControllerCaps) -> c_int;

// ---------------------------------------------------------------------------
// AdlLibrary — the main handle
// ---------------------------------------------------------------------------

/// Dynamic wrapper around `atiadlxx.dll`.
///
/// Holds the `libloading::Library` (keeping the DLL mapped), the ADL2 context,
/// and resolved function pointers for every ADL call we use.
pub struct AdlLibrary {
    _lib: Arc<libloading::Library>,
    context: *mut c_void,
    fn_destroy: FnAdl2MainControlDestroy,
    fn_adapter_count: FnAdl2AdapterNumberGet,
    fn_adapter_info: FnAdl2AdapterInfoGet,
    fn_adapter_active: FnAdl2AdapterActiveGet,
    // Overdrive 6 sensor functions — may be None if the symbols are absent
    fn_od6_temperature: Option<FnAdl2OD6TemperatureGet>,
    fn_od6_fan_speed: Option<FnAdl2OD6FanSpeedGet>,
    #[allow(dead_code)] // reserved for future thermal-cap queries
    fn_od6_thermal_caps: Option<FnAdl2OD6ThermalCaps>,
}

// SAFETY: The ADL2 library is thread-safe once initialised via its own context.
unsafe impl Send for AdlLibrary {}

impl AdlLibrary {
    /// Try to load the ADL shared library and initialise it.
    ///
    /// Returns `None` if `atiadlxx.dll` cannot be found (no AMD driver)
    /// or if `ADL2_Main_Control_Create` fails.
    pub fn try_load() -> Option<Self> {
        // SAFETY: We are loading a well-known system DLL.
        let lib = unsafe { libloading::Library::new("atiadlxx.dll") }.ok()?;

        unsafe {
            // Resolve mandatory symbols
            let fn_create: FnAdl2MainControlCreate =
                *lib.get(b"ADL2_Main_Control_Create\0").ok()?;
            let fn_destroy: FnAdl2MainControlDestroy =
                *lib.get(b"ADL2_Main_Control_Destroy\0").ok()?;
            let fn_adapter_count: FnAdl2AdapterNumberGet =
                *lib.get(b"ADL2_Adapter_NumberOfAdapters_Get\0").ok()?;
            let fn_adapter_info: FnAdl2AdapterInfoGet =
                *lib.get(b"ADL2_Adapter_AdapterInfo_Get\0").ok()?;
            let fn_adapter_active: FnAdl2AdapterActiveGet =
                *lib.get(b"ADL2_Adapter_Active_Get\0").ok()?;

            // Overdrive 6 symbols are optional — older drivers may lack them.
            let fn_od6_temperature: Option<FnAdl2OD6TemperatureGet> = lib
                .get(b"ADL2_Overdrive6_Temperature_Get\0")
                .ok()
                .map(|s| *s);
            let fn_od6_fan_speed: Option<FnAdl2OD6FanSpeedGet> =
                lib.get(b"ADL2_Overdrive6_FanSpeed_Get\0").ok().map(|s| *s);
            let fn_od6_thermal_caps: Option<FnAdl2OD6ThermalCaps> = lib
                .get(b"ADL2_Overdrive6_ThermalController_Caps\0")
                .ok()
                .map(|s| *s);

            // Initialise ADL2 context.
            // Second parameter: 1 = enumerate only active/connected adapters.
            let mut context: *mut c_void = std::ptr::null_mut();
            let ret = fn_create(adl_malloc, 1, &mut context);
            if ret != ADL_OK {
                log::warn!("ADL2_Main_Control_Create failed with error code {ret}");
                return None;
            }

            Some(Self {
                _lib: Arc::new(lib),
                context,
                fn_destroy,
                fn_adapter_count,
                fn_adapter_info,
                fn_adapter_active,
                fn_od6_temperature,
                fn_od6_fan_speed,
                fn_od6_thermal_caps,
            })
        }
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    fn read_c_string(buf: &[c_char]) -> String {
        // SAFETY: buffer is null-terminated by ADL.
        unsafe { CStr::from_ptr(buf.as_ptr()) }
            .to_string_lossy()
            .into_owned()
    }

    /// Public helper to extract a C string from an adapter info field.
    pub fn read_c_string_pub(buf: &[c_char]) -> String {
        Self::read_c_string(buf)
    }

    // -----------------------------------------------------------------------
    // Public safe wrappers
    // -----------------------------------------------------------------------

    /// Number of adapters reported by the AMD driver (includes inactive).
    pub fn adapter_count(&self) -> Result<i32, AdlError> {
        let mut count: c_int = 0;
        let ret = unsafe { (self.fn_adapter_count)(self.context, &mut count) };
        adl_check(ret)?;
        Ok(count)
    }

    /// Retrieve adapter info for all adapters.
    pub fn adapter_info(&self, count: i32) -> Result<Vec<AdlAdapterInfo>, AdlError> {
        let mut infos: Vec<AdlAdapterInfo> = vec![AdlAdapterInfo::default(); count as usize];
        let size = (count as usize) * std::mem::size_of::<AdlAdapterInfo>();
        let ret =
            unsafe { (self.fn_adapter_info)(self.context, infos.as_mut_ptr(), size as c_int) };
        adl_check(ret)?;
        Ok(infos)
    }

    /// Check if an adapter is currently active.
    pub fn adapter_active(&self, adapter_index: i32) -> Result<bool, AdlError> {
        let mut status: c_int = 0;
        let ret = unsafe { (self.fn_adapter_active)(self.context, adapter_index, &mut status) };
        adl_check(ret)?;
        Ok(status == ADL_TRUE)
    }

    /// GPU temperature in degrees Celsius (via Overdrive 6).
    /// Returns `None` if the OD6 temperature function is not available.
    pub fn temperature_celsius(&self, adapter_index: i32) -> Option<f64> {
        let func = self.fn_od6_temperature?;
        let mut milli_deg: c_int = 0;
        let ret = unsafe { func(self.context, adapter_index, &mut milli_deg) };
        if ret != ADL_OK {
            return None;
        }
        Some(milli_deg as f64 / 1000.0)
    }

    /// Fan speed as a percentage (via Overdrive 6).
    /// Returns `None` if the OD6 fan-speed function is not available.
    pub fn fan_speed_percent(&self, adapter_index: i32) -> Option<f64> {
        let func = self.fn_od6_fan_speed?;
        let mut info = AdlOD6FanSpeedInfo::default();
        let ret = unsafe { func(self.context, adapter_index, &mut info) };
        if ret != ADL_OK {
            return None;
        }
        Some(info.fan_speed_percent as f64)
    }

    /// Fan speed in RPM (via Overdrive 6).
    /// Returns `None` if the OD6 fan-speed function is not available.
    pub fn fan_speed_rpm(&self, adapter_index: i32) -> Option<f64> {
        let func = self.fn_od6_fan_speed?;
        let mut info = AdlOD6FanSpeedInfo::default();
        let ret = unsafe { func(self.context, adapter_index, &mut info) };
        if ret != ADL_OK {
            return None;
        }
        Some(info.fan_speed_rpm as f64)
    }
}

impl Drop for AdlLibrary {
    fn drop(&mut self) {
        if !self.context.is_null() {
            let ret = unsafe { (self.fn_destroy)(self.context) };
            if ret != ADL_OK {
                log::warn!("ADL2_Main_Control_Destroy returned error code {ret}");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct AdlError {
    pub code: i32,
}

impl std::fmt::Display for AdlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ADL returned error code {}", self.code)
    }
}

impl std::error::Error for AdlError {}

fn adl_check(ret: c_int) -> Result<(), AdlError> {
    if ret == ADL_OK {
        Ok(())
    } else {
        Err(AdlError { code: ret })
    }
}
