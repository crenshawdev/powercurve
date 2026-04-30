// SPDX-License-Identifier: GPL-3.0-only

use std::ffi::{CStr, c_void};
use std::os::raw::c_uint;

/// NVML return code. 0 means success.
type NvmlReturn = c_uint;
const NVML_SUCCESS: NvmlReturn = 0;

/// Opaque device handle from NVML.
type NvmlDevice = *mut c_void;

/// GPU die temperature sensor.
const NVML_TEMPERATURE_GPU: c_uint = 0;

// Function pointer types for the NVML calls we need.
type InitFn = unsafe extern "C" fn() -> NvmlReturn;
type ShutdownFn = unsafe extern "C" fn() -> NvmlReturn;
type GetCountFn = unsafe extern "C" fn(*mut c_uint) -> NvmlReturn;
type GetHandleFn = unsafe extern "C" fn(c_uint, *mut NvmlDevice) -> NvmlReturn;
type GetTempFn = unsafe extern "C" fn(NvmlDevice, c_uint, *mut c_uint) -> NvmlReturn;

/// NVIDIA GPU temperature source state.
pub enum NvidiaState {
    /// No NVIDIA hardware on the PCI bus.
    Absent,
    /// NVIDIA hardware detected but NVML failed to load. Safety fallback active.
    Unavailable,
    /// NVML loaded and ready for temperature queries.
    Active(NvmlHandle),
}

/// Handle to the NVML library loaded at runtime via dlopen.
///
/// Caches device handles at init since GPUs don't hot-swap on desktops.
/// Calls nvmlShutdown and dlclose on drop.
pub struct NvmlHandle {
    lib: *mut c_void,
    shutdown: ShutdownFn,
    get_temp: GetTempFn,
    devices: Vec<NvmlDevice>,
}

/// Resolve a symbol from a dlopen'd library.
///
/// # Safety
/// Caller must ensure `lib` is a valid handle from dlopen and that the
/// symbol's actual signature matches the target type `T`.
unsafe fn resolve<T>(lib: *mut c_void, name: &CStr) -> Option<T> {
    unsafe {
        let sym = libc::dlsym(lib, name.as_ptr());
        if sym.is_null() {
            return None;
        }
        Some(std::mem::transmute_copy(&sym))
    }
}

impl NvmlHandle {
    /// Load libnvidia-ml.so.1, initialize NVML, and enumerate GPU devices.
    ///
    /// Returns None if the library can't be loaded, init fails, or no
    /// devices are found. Each failure is logged with the specific reason.
    pub fn open() -> Option<Self> {
        // SAFETY: dlopen with a bare library name and RTLD_LAZY is always
        // safe to call. The returned pointer is checked for null immediately.
        // dlopen the library
        let lib = unsafe { libc::dlopen(c"libnvidia-ml.so.1".as_ptr(), libc::RTLD_LAZY) };
        if lib.is_null() {
            log::warn!("nvml: failed to load libnvidia-ml.so.1");
            return None;
        }

        // SAFETY: lib is a non-null handle confirmed above. Each symbol name
        // matches NVML's documented ABI for that entry point. The resolved
        // function pointers are valid for the lifetime of lib, and lib is kept
        // alive in Self until Drop. dlclose on early-return paths is safe
        // because no resolved pointers escape before we reach Some(Self{...}).
        // Resolve all the symbols we need
        let init: InitFn = match unsafe { resolve(lib, c"nvmlInit_v2") } {
            Some(f) => f,
            None => {
                log::warn!("nvml: missing symbol nvmlInit_v2");
                unsafe { libc::dlclose(lib) };
                return None;
            }
        };

        let shutdown: ShutdownFn = match unsafe { resolve(lib, c"nvmlShutdown") } {
            Some(f) => f,
            None => {
                log::warn!("nvml: missing symbol nvmlShutdown");
                unsafe { libc::dlclose(lib) };
                return None;
            }
        };

        let get_count: GetCountFn = match unsafe { resolve(lib, c"nvmlDeviceGetCount_v2") } {
            Some(f) => f,
            None => {
                log::warn!("nvml: missing symbol nvmlDeviceGetCount_v2");
                unsafe { libc::dlclose(lib) };
                return None;
            }
        };

        let get_handle: GetHandleFn =
            match unsafe { resolve(lib, c"nvmlDeviceGetHandleByIndex_v2") } {
                Some(f) => f,
                None => {
                    log::warn!("nvml: missing symbol nvmlDeviceGetHandleByIndex_v2");
                    unsafe { libc::dlclose(lib) };
                    return None;
                }
            };

        let get_temp: GetTempFn = match unsafe { resolve(lib, c"nvmlDeviceGetTemperature") } {
            Some(f) => f,
            None => {
                log::warn!("nvml: missing symbol nvmlDeviceGetTemperature");
                unsafe { libc::dlclose(lib) };
                return None;
            }
        };

        // SAFETY: init is a resolved, non-null function pointer whose
        // signature matches nvmlInit_v2. The library is loaded and live.
        // Initialize NVML
        let ret = unsafe { init() };
        if ret != NVML_SUCCESS {
            log::warn!("nvml: nvmlInit_v2 failed with error {ret}");
            unsafe { libc::dlclose(lib) };
            return None;
        }

        // SAFETY: get_count and get_handle are resolved, non-null function
        // pointers matching their respective NVML ABI signatures. NVML is
        // initialized at this point. The mutable references passed are local
        // stack variables so there are no aliasing hazards.
        // Enumerate devices
        let mut count: c_uint = 0;
        let ret = unsafe { get_count(&mut count) };
        if ret != NVML_SUCCESS {
            log::warn!("nvml: nvmlDeviceGetCount_v2 failed with error {ret}");
            unsafe { shutdown() };
            unsafe { libc::dlclose(lib) };
            return None;
        }

        if count == 0 {
            log::warn!("nvml: no devices found");
            unsafe { shutdown() };
            unsafe { libc::dlclose(lib) };
            return None;
        }

        let mut devices = Vec::with_capacity(count as usize);
        for i in 0..count {
            let mut handle: NvmlDevice = std::ptr::null_mut();
            let ret = unsafe { get_handle(i, &mut handle) };
            if ret != NVML_SUCCESS {
                log::warn!("nvml: failed to get handle for device {i}: error {ret}");
                continue;
            }
            devices.push(handle);
        }

        if devices.is_empty() {
            log::warn!("nvml: could not get handle for any device");
            unsafe { shutdown() };
            unsafe { libc::dlclose(lib) };
            return None;
        }

        Some(Self { lib, shutdown, get_temp, devices })
    }

    /// Highest temperature across all NVIDIA GPUs, in millidegrees Celsius.
    ///
    /// Returns None if every device fails to report. Individual failures are
    /// logged at debug level and don't prevent retries on the next cycle.
    pub fn max_gpu_temp(&self) -> Option<u32> {
        let mut best: Option<u32> = None;

        for (i, &device) in self.devices.iter().enumerate() {
            let mut temp: c_uint = 0;
            // SAFETY: get_temp is a valid function pointer for the lifetime of
            // self (lib is live). device was obtained from nvmlDeviceGetHandleByIndex
            // and remains valid while the library is loaded. temp is a local
            // stack variable with no aliasing.
            let ret = unsafe { (self.get_temp)(device, NVML_TEMPERATURE_GPU, &mut temp) };

            if ret == NVML_SUCCESS {
                let millideg = temp * 1000;
                if best.is_none_or(|b| millideg > b) {
                    best = Some(millideg);
                }
            } else {
                log::debug!("nvml: failed to read temp for device {i}: error {ret}");
            }
        }

        best
    }

    /// Number of GPU devices NVML found.
    pub fn device_count(&self) -> usize {
        self.devices.len()
    }
}

impl Drop for NvmlHandle {
    fn drop(&mut self) {
        // SAFETY: shutdown is a valid function pointer and NVML is still
        // initialized at drop time. lib is the non-null handle from dlopen;
        // dlclose is safe here because no function pointers from this handle
        // escape Self, so nothing will call into the unloaded library after
        // this point.
        unsafe {
            (self.shutdown)();
            libc::dlclose(self.lib);
        }
    }
}
