//! Attribution of scroll events to specific physical devices via
//! IOHIDManager.
//!
//! A CGEvent carries no device identity, so the event tap alone cannot tell
//! two mice apart. IOHIDManager can: every discrete wheel tick also arrives
//! as a HID value from a concrete device with a vendor/product ID. Both
//! callbacks run on the same run loop thread, so "the device that most
//! recently sent a wheel value" is the device whose CGEvent the tap is
//! processing right now.
//!
//! Honest limits: this covers DISCRETE wheels only. Magic Mouse and
//! trackpad scrolling is synthesized from touch data and never appears as
//! a HID Wheel/AC Pan value, so continuous scrolling stays unattributed
//! and per-device rules do not apply to it. If two mice emit wheel ticks
//! within the same attribution window, the later one wins - a documented
//! heuristic, same family as other per-device scroll utilities use.
//!
//! All FFI signatures below were verified against this machine's SDK
//! headers (IOHIDManager.h, IOHIDValue.h, IOHIDElement.h, IOHIDDevice.h,
//! IOHIDDeviceKeys.h, IOHIDUsageTables.h), not assumed.

use std::ffi::c_void;
use std::ptr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use core_foundation::base::{CFType, CFTypeRef, TCFType};
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::number::CFNumber;
use core_foundation::runloop::CFRunLoop;
use core_foundation::string::{CFString, CFStringRef};
use core_foundation_sys::base::{CFIndex, CFRelease};
use core_foundation_sys::runloop::{CFRunLoopRef, kCFRunLoopDefaultMode};
use core_foundation_sys::set::{CFSetGetCount, CFSetGetValues, CFSetRef};

use crate::device::HardwareId;
use crate::error::{AppError, AppResult};

type IOHIDManagerRef = *mut c_void;
type IOHIDDeviceRef = *mut c_void;
type IOHIDElementRef = *mut c_void;
type IOHIDValueRef = *mut c_void;
type IOReturn = i32;
type IOOptionBits = u32;

const KIO_RETURN_SUCCESS: IOReturn = 0;
const KIOHID_OPTIONS_TYPE_NONE: IOOptionBits = 0;

// Verified from IOHIDUsageTables.h / IOHIDDeviceKeys.h.
const HID_PAGE_GENERIC_DESKTOP: u32 = 0x01;
const HID_USAGE_GD_MOUSE: u32 = 0x02;
const HID_USAGE_GD_WHEEL: u32 = 0x38;
const HID_PAGE_CONSUMER: u32 = 0x0C;
const HID_USAGE_CSMR_AC_PAN: u32 = 0x238;
const KEY_DEVICE_USAGE_PAGE: &str = "DeviceUsagePage";
const KEY_DEVICE_USAGE: &str = "DeviceUsage";
const KEY_VENDOR_ID: &str = "VendorID";
const KEY_PRODUCT_ID: &str = "ProductID";
const KEY_PRODUCT: &str = "Product";
const KEY_TRANSPORT: &str = "Transport";

// Plain extern "C", NOT "C-unwind": IOHIDManager invokes this from C
// dispatch inside a CFRunLoop callout. A Rust panic must not unwind across
// those C frames (undefined behavior); extern "C" turns a panic into a
// clean, deterministic abort instead. Nothing above the callback could
// catch an unwind anyway, so "C-unwind" would buy nothing.
type ValueCallback = extern "C" fn(*mut c_void, IOReturn, *mut c_void, IOHIDValueRef);

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOHIDManagerCreate(allocator: *const c_void, options: IOOptionBits) -> IOHIDManagerRef;
    fn IOHIDManagerSetDeviceMatching(manager: IOHIDManagerRef, matching: CFDictionaryRef);
    fn IOHIDManagerRegisterInputValueCallback(
        manager: IOHIDManagerRef,
        callback: ValueCallback,
        context: *mut c_void,
    );
    fn IOHIDManagerScheduleWithRunLoop(
        manager: IOHIDManagerRef,
        run_loop: CFRunLoopRef,
        mode: CFStringRef,
    );
    fn IOHIDManagerUnscheduleFromRunLoop(
        manager: IOHIDManagerRef,
        run_loop: CFRunLoopRef,
        mode: CFStringRef,
    );
    fn IOHIDManagerOpen(manager: IOHIDManagerRef, options: IOOptionBits) -> IOReturn;
    fn IOHIDManagerCopyDevices(manager: IOHIDManagerRef) -> CFSetRef;
    fn IOHIDDeviceGetProperty(device: IOHIDDeviceRef, key: CFStringRef) -> CFTypeRef;
    fn IOHIDValueGetElement(value: IOHIDValueRef) -> IOHIDElementRef;
    fn IOHIDValueGetIntegerValue(value: IOHIDValueRef) -> CFIndex;
    fn IOHIDElementGetUsagePage(element: IOHIDElementRef) -> u32;
    fn IOHIDElementGetUsage(element: IOHIDElementRef) -> u32;
    fn IOHIDElementGetDevice(element: IOHIDElementRef) -> IOHIDDeviceRef;
}

/// The most recent wheel tick seen from any matched device, plus that
/// device's shared `Product` name (read from the same `IOHIDDeviceRef` the
/// callback already has in hand - no extra IOHIDManager call). Written by
/// the HID callback, read by the event tap callback; both run on the main
/// run loop thread, so the Mutex is uncontended - it exists to satisfy
/// `static` soundness, not for real cross-thread traffic.
struct RecentWheel {
    hardware: HardwareId,
    name: Option<Arc<str>>,
    at: Instant,
}

static LAST_WHEEL: Mutex<Option<RecentWheel>> = Mutex::new(None);

/// One consistent view of the last attributed wheel tick. Returning the
/// hardware id and display name together avoids taking `LAST_WHEEL` twice
/// for one CGEvent and guarantees both fields came from the same HID tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WheelSnapshot {
    pub hardware: HardwareId,
    pub name: Option<Arc<str>>,
}

/// Starts a mouse-matching HID monitor on the CURRENT thread's run loop.
/// Must be called on the same thread that will run the event tap loop,
/// before entering it. On success the manager intentionally lives for the
/// rest of the process; on failure it is cleaned up before returning.
pub fn start_wheel_monitor() -> AppResult<()> {
    unsafe {
        let manager = IOHIDManagerCreate(ptr::null(), KIOHID_OPTIONS_TYPE_NONE);
        if manager.is_null() {
            return Err(AppError::Platform(
                "IOHIDManagerCreate returned NULL".to_string(),
            ));
        }

        let run_loop = CFRunLoop::get_current().as_concrete_TypeRef();
        let matching = mouse_matching_dictionary();
        IOHIDManagerSetDeviceMatching(manager, matching.as_concrete_TypeRef());
        IOHIDManagerRegisterInputValueCallback(manager, wheel_value_callback, ptr::null_mut());
        IOHIDManagerScheduleWithRunLoop(manager, run_loop, kCFRunLoopDefaultMode);

        let status = IOHIDManagerOpen(manager, KIOHID_OPTIONS_TYPE_NONE);
        if status != KIO_RETURN_SUCCESS {
            // Undo the scheduling and release the manager: on this error path
            // (usually a denied Input Monitoring grant) the caller keeps
            // running in degraded per-kind mode, so a dead manager left
            // attached to the run loop would leak for the whole process.
            IOHIDManagerUnscheduleFromRunLoop(manager, run_loop, kCFRunLoopDefaultMode);
            CFRelease(manager as CFTypeRef);
            return Err(AppError::Platform(format!(
                "IOHIDManagerOpen failed with IOReturn {status:#010x}; Input Monitoring \
                 permission is the usual cause"
            )));
        }
        // Success: the manager intentionally lives for the rest of the
        // process (a `run` invocation never returns), so it is not released.
    }
    Ok(())
}

/// The device that produced a wheel tick within `max_age`, if any. The
/// product name is captured by the same callback as the hardware id.
pub fn recent_wheel_snapshot(max_age: Duration) -> Option<WheelSnapshot> {
    let last = LAST_WHEEL.lock().ok()?;
    last.as_ref()
        .filter(|recent| recent.at.elapsed() <= max_age)
        .map(|recent| WheelSnapshot {
            hardware: recent.hardware,
            name: recent.name.clone(),
        })
}

extern "C" fn wheel_value_callback(
    _context: *mut c_void,
    result: IOReturn,
    _sender: *mut c_void,
    value: IOHIDValueRef,
) {
    if result != KIO_RETURN_SUCCESS || value.is_null() {
        return;
    }

    unsafe {
        let element = IOHIDValueGetElement(value);
        if element.is_null() {
            return;
        }

        let page = IOHIDElementGetUsagePage(element);
        let usage = IOHIDElementGetUsage(element);
        let is_wheel = (page == HID_PAGE_GENERIC_DESKTOP && usage == HID_USAGE_GD_WHEEL)
            || (page == HID_PAGE_CONSUMER && usage == HID_USAGE_CSMR_AC_PAN);
        if !is_wheel || IOHIDValueGetIntegerValue(value) == 0 {
            return;
        }

        let device = IOHIDElementGetDevice(element);
        if device.is_null() {
            return;
        }

        if let Some(hardware) = hardware_id_of(device)
            && let Ok(mut last) = LAST_WHEEL.lock()
        {
            // Product names are stable for a connected device. Reuse the
            // existing Arc for consecutive ticks instead of cloning and then
            // re-wrapping a String for every CGEvent diagnostics row.
            let name = match last.as_ref() {
                Some(previous) if previous.hardware == hardware => previous.name.clone(),
                _ => string_property(device, KEY_PRODUCT).map(Arc::<str>::from),
            };
            *last = Some(RecentWheel {
                hardware,
                name,
                at: Instant::now(),
            });
        }
    }
}

/// A connected pointing device, as shown by the `devices` CLI command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub hardware: HardwareId,
    pub name: Option<String>,
    pub transport: Option<String>,
}

/// Enumerates currently connected mouse-usage HID devices. Uses its own
/// short-lived manager so the CLI command works without starting the
/// monitor; property reads do not require opening the devices.
pub fn list_pointing_devices() -> AppResult<Vec<DeviceInfo>> {
    unsafe {
        let manager = IOHIDManagerCreate(ptr::null(), KIOHID_OPTIONS_TYPE_NONE);
        if manager.is_null() {
            return Err(AppError::Platform(
                "IOHIDManagerCreate returned NULL".to_string(),
            ));
        }

        let matching = mouse_matching_dictionary();
        IOHIDManagerSetDeviceMatching(manager, matching.as_concrete_TypeRef());

        let device_set = IOHIDManagerCopyDevices(manager);
        let mut devices = Vec::new();
        if !device_set.is_null() {
            let count = CFSetGetCount(device_set) as usize;
            let mut refs: Vec<*const c_void> = vec![ptr::null(); count];
            CFSetGetValues(device_set, refs.as_mut_ptr());
            for device in refs {
                let device = device as IOHIDDeviceRef;
                if let Some(hardware) = hardware_id_of(device) {
                    devices.push(DeviceInfo {
                        hardware,
                        name: string_property(device, KEY_PRODUCT),
                        transport: string_property(device, KEY_TRANSPORT),
                    });
                }
            }
            CFRelease(device_set as CFTypeRef);
        }
        CFRelease(manager as CFTypeRef);

        devices.sort_by_key(|d| (d.hardware.vendor_id, d.hardware.product_id));
        devices.dedup();
        Ok(devices)
    }
}

fn mouse_matching_dictionary() -> CFDictionary<CFString, CFNumber> {
    CFDictionary::from_CFType_pairs(&[
        (
            CFString::from_static_string(KEY_DEVICE_USAGE_PAGE),
            CFNumber::from(HID_PAGE_GENERIC_DESKTOP as i32),
        ),
        (
            CFString::from_static_string(KEY_DEVICE_USAGE),
            CFNumber::from(HID_USAGE_GD_MOUSE as i32),
        ),
    ])
}

unsafe fn hardware_id_of(device: IOHIDDeviceRef) -> Option<HardwareId> {
    let vendor_id = unsafe { number_property(device, KEY_VENDOR_ID)? };
    let product_id = unsafe { number_property(device, KEY_PRODUCT_ID)? };
    Some(HardwareId {
        vendor_id: u32::try_from(vendor_id).ok()?,
        product_id: u32::try_from(product_id).ok()?,
    })
}

unsafe fn number_property(device: IOHIDDeviceRef, key: &'static str) -> Option<i64> {
    unsafe {
        let key = CFString::from_static_string(key);
        let value = IOHIDDeviceGetProperty(device, key.as_concrete_TypeRef());
        if value.is_null() {
            return None;
        }
        // Get rule: the device still owns the property; wrap_under_get_rule
        // retains, the CFType drop releases our retain only.
        CFType::wrap_under_get_rule(value)
            .downcast::<CFNumber>()?
            .to_i64()
    }
}

unsafe fn string_property(device: IOHIDDeviceRef, key: &'static str) -> Option<String> {
    unsafe {
        let key = CFString::from_static_string(key);
        let value = IOHIDDeviceGetProperty(device, key.as_concrete_TypeRef());
        if value.is_null() {
            return None;
        }
        Some(
            CFType::wrap_under_get_rule(value)
                .downcast::<CFString>()?
                .to_string(),
        )
    }
}
