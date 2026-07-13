//! Attribution of scroll events to specific physical devices via
//! IOHIDManager.
//!
//! A CGEvent carries no device identity, so the event tap alone cannot tell
//! two mice apart. IOHIDManager can: every discrete wheel tick also arrives
//! as a HID value from a concrete device with a vendor/product ID. Both
//! callbacks run on the same run loop thread, so a very recent wheel value is
//! useful correlation evidence for the following CGEvent. It is not an exact
//! device identifier; `device_attribution` applies confidence and timeout.
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
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
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

use crate::device::{DeviceIdentity, HardwareId};
use crate::device_catalog::ObservedDevice;
use crate::device_classifier::ContinuousSourceHint;
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
const KEY_SERIAL_NUMBER: &str = "SerialNumber";
const KEY_LOCATION_ID: &str = "LocationID";
const KEY_TRANSPORT: &str = "Transport";
const IDENTITY_CACHE_MAX_IDLE: Duration = Duration::from_secs(5);

// Plain extern "C", NOT "C-unwind": IOHIDManager invokes this from C
// dispatch inside a CFRunLoop callout. A Rust panic must not unwind across
// those C frames (undefined behavior); extern "C" turns a panic into a
// clean, deterministic abort instead. Nothing above the callback could
// catch an unwind anyway, so "C-unwind" would buy nothing.
type ValueCallback = extern "C" fn(*mut c_void, IOReturn, *mut c_void, IOHIDValueRef);
type DeviceCallback = extern "C" fn(*mut c_void, IOReturn, *mut c_void, IOHIDDeviceRef);

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOHIDManagerCreate(allocator: *const c_void, options: IOOptionBits) -> IOHIDManagerRef;
    fn IOHIDManagerSetDeviceMatching(manager: IOHIDManagerRef, matching: CFDictionaryRef);
    fn IOHIDManagerRegisterInputValueCallback(
        manager: IOHIDManagerRef,
        callback: ValueCallback,
        context: *mut c_void,
    );
    fn IOHIDManagerRegisterDeviceMatchingCallback(
        manager: IOHIDManagerRef,
        callback: DeviceCallback,
        context: *mut c_void,
    );
    fn IOHIDManagerRegisterDeviceRemovalCallback(
        manager: IOHIDManagerRef,
        callback: DeviceCallback,
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

/// The most recent wheel tick seen from any matched device, plus its cached
/// identity, shared `Product` name, and public `Transport` value (read from
/// the same `IOHIDDeviceRef` the callback already has in hand). Written by
/// the HID callback, read by the event tap callback; both run on the main
/// run loop thread, so the Mutex is uncontended - it exists to satisfy
/// `static` soundness, not for real cross-thread traffic.
struct RecentWheel {
    device_token: usize,
    identity: Option<Arc<DeviceIdentity>>,
    name: Option<Arc<str>>,
    transport: Option<Arc<str>>,
    at: Instant,
}

static LAST_WHEEL: Mutex<Option<RecentWheel>> = Mutex::new(None);
const LIVE_HINT_TRACKPAD_ONLY: u8 = 1;
const LIVE_HINT_MAGIC_MOUSE_ONLY: u8 = 2;
const LIVE_HINT_BOTH: u8 = 3;
const LIVE_HINT_UNKNOWN: u8 = 4;
static LIVE_CONTINUOUS_HINT: AtomicU8 = AtomicU8::new(0);
static LIVE_CONTINUOUS_HINT_READY: AtomicBool = AtomicBool::new(false);

/// One consistent view of the last attributed wheel tick. Returning the
/// identity, display name, and transport together avoids taking `LAST_WHEEL`
/// more than once for one CGEvent and guarantees every field came from the
/// same HID tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WheelSnapshot {
    pub identity: Option<Arc<DeviceIdentity>>,
    pub name: Option<Arc<str>>,
    pub transport: Option<Arc<str>>,
    pub age: Duration,
}

/// Starts a mouse-matching HID monitor on the CURRENT thread's run loop.
/// Must be called on the same thread that will run the event tap loop,
/// before entering it. On success the manager intentionally lives for the
/// rest of the process; on failure it is cleaned up before returning.
pub fn start_wheel_monitor() -> AppResult<()> {
    LIVE_CONTINUOUS_HINT_READY.store(false, Ordering::Release);
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
        // The manager already tracks all generic mouse-usage devices for wheel
        // attribution. Reuse its lifecycle notifications to keep the cheap
        // continuous-device hint current across hot-plug and sleep/wake.
        IOHIDManagerRegisterDeviceMatchingCallback(
            manager,
            pointing_device_inventory_changed,
            manager,
        );
        IOHIDManagerRegisterDeviceRemovalCallback(
            manager,
            pointing_device_inventory_changed,
            manager,
        );
        IOHIDManagerScheduleWithRunLoop(manager, run_loop, kCFRunLoopDefaultMode);

        let status = IOHIDManagerOpen(manager, KIOHID_OPTIONS_TYPE_NONE);
        if status != KIO_RETURN_SUCCESS {
            // Undo the scheduling and release the manager. The caller keeps
            // running in degraded per-kind mode, so a dead manager left
            // attached to the run loop would leak for the whole process.
            IOHIDManagerUnscheduleFromRunLoop(manager, run_loop, kCFRunLoopDefaultMode);
            CFRelease(manager as CFTypeRef);
            LIVE_CONTINUOUS_HINT_READY.store(false, Ordering::Release);
            return Err(AppError::Platform(format!(
                "IOHIDManagerOpen failed with IOReturn {status:#010x}; per-device attribution \
                 is unavailable"
            )));
        }
        refresh_live_continuous_hint(manager);
        // Success: the manager intentionally lives for the rest of the
        // process (a `run` invocation never returns), so it is not released.
    }
    Ok(())
}

/// The device that produced a wheel tick within `max_age`, if any. The
/// Product name and transport are captured by the same callback as identity.
pub fn recent_wheel_snapshot(max_age: Duration) -> Option<WheelSnapshot> {
    let last = LAST_WHEEL.lock().ok()?;
    last.as_ref().and_then(|recent| {
        let age = recent.at.elapsed();
        (age <= max_age).then(|| WheelSnapshot {
            identity: recent.identity.clone(),
            name: recent.name.clone(),
            transport: recent.transport.clone(),
            age,
        })
    })
}

/// Lock-free current inventory for the scroll-event hot path. `None` means
/// the long-lived HID monitor did not start, so the caller should keep the
/// one-shot hint it obtained during setup.
pub fn live_continuous_source_hint() -> Option<ContinuousSourceHint> {
    if !LIVE_CONTINUOUS_HINT_READY.load(Ordering::Acquire) {
        return None;
    }
    match LIVE_CONTINUOUS_HINT.load(Ordering::Acquire) {
        LIVE_HINT_TRACKPAD_ONLY => Some(ContinuousSourceHint::TrackpadOnly),
        LIVE_HINT_MAGIC_MOUSE_ONLY => Some(ContinuousSourceHint::MagicMouseOnly),
        LIVE_HINT_BOTH => Some(ContinuousSourceHint::Both),
        LIVE_HINT_UNKNOWN => Some(ContinuousSourceHint::Unknown),
        _ => None,
    }
}

extern "C" fn pointing_device_inventory_changed(
    context: *mut c_void,
    result: IOReturn,
    _sender: *mut c_void,
    _device: IOHIDDeviceRef,
) {
    if result != KIO_RETURN_SUCCESS || context.is_null() {
        return;
    }
    unsafe { refresh_live_continuous_hint(context as IOHIDManagerRef) };
}

unsafe fn refresh_live_continuous_hint(manager: IOHIDManagerRef) {
    let snapshots = unsafe { mouse_device_snapshots_from_manager(manager) };
    let hint = continuous_source_hint_from_snapshots(&snapshots);
    let encoded = match hint {
        ContinuousSourceHint::TrackpadOnly => LIVE_HINT_TRACKPAD_ONLY,
        ContinuousSourceHint::MagicMouseOnly => LIVE_HINT_MAGIC_MOUSE_ONLY,
        ContinuousSourceHint::Both => LIVE_HINT_BOTH,
        ContinuousSourceHint::Unknown => LIVE_HINT_UNKNOWN,
    };
    LIVE_CONTINUOUS_HINT.store(encoded, Ordering::Release);
    LIVE_CONTINUOUS_HINT_READY.store(true, Ordering::Release);
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

        if let Ok(mut last) = LAST_WHEEL.lock() {
            let device_token = device as usize;
            // All IOKit identity/name properties are stable while this
            // IOHIDDeviceRef is connected. Reuse their Arcs on consecutive
            // ticks so the event hot path performs no string allocation.
            let cached = last
                .as_ref()
                .filter(|previous| {
                    previous.device_token == device_token
                        && previous.at.elapsed() <= IDENTITY_CACHE_MAX_IDLE
                })
                .map(|previous| {
                    (
                        previous.identity.clone(),
                        previous.name.clone(),
                        previous.transport.clone(),
                    )
                });
            let (identity, name, transport) = if let Some(cached) = cached {
                cached
            } else {
                (
                    device_identity_of(device).map(Arc::new),
                    string_property(device, KEY_PRODUCT).map(Arc::<str>::from),
                    string_property(device, KEY_TRANSPORT).map(Arc::<str>::from),
                )
            };
            *last = Some(RecentWheel {
                device_token,
                identity,
                name,
                transport,
                at: Instant::now(),
            });
        }
    }
}

/// A connected pointing device, as shown by the `devices` CLI command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub identity: DeviceIdentity,
    pub name: Option<String>,
    pub transport: Option<String>,
}

#[derive(Debug)]
struct MouseDeviceSnapshot {
    identity: Option<DeviceIdentity>,
    name: Option<String>,
    transport: Option<String>,
}

/// Connected continuous-source inventory used before the timing heuristic.
/// Product names come from public IOHID properties and remain useful even for
/// the built-in trackpad, whose generic mouse interface can omit the numeric
/// identity fields required by per-device wheel rules.
pub fn continuous_source_hint() -> AppResult<ContinuousSourceHint> {
    Ok(continuous_source_hint_from_snapshots(
        &mouse_device_snapshots()?,
    ))
}

fn continuous_source_hint_from_snapshots(
    snapshots: &[MouseDeviceSnapshot],
) -> ContinuousSourceHint {
    let mut trackpad = false;
    let mut magic_mouse = false;
    for device in snapshots {
        let Some(name) = device.name.as_deref() else {
            continue;
        };
        let (is_trackpad, is_magic_mouse) = continuous_device_name_flags(name);
        trackpad |= is_trackpad;
        magic_mouse |= is_magic_mouse;
    }
    ContinuousSourceHint::from_presence(trackpad, magic_mouse)
}

/// Enumerates currently connected mouse-usage HID devices. Uses its own
/// short-lived manager so the CLI command works without starting the
/// monitor; property reads do not require opening the devices.
pub fn list_pointing_devices() -> AppResult<Vec<DeviceInfo>> {
    let mut devices: Vec<_> = list_pointing_device_observations()?
        .into_iter()
        .filter_map(|snapshot| {
            Some(DeviceInfo {
                identity: snapshot.identity?,
                name: snapshot.name,
                transport: snapshot.transport,
            })
        })
        .collect();

    devices.sort_by(|left, right| left.identity.cmp(&right.identity));
    devices.dedup_by(|left, right| left.identity == right.identity);
    Ok(devices)
}

/// Enumerates every connected mouse-usage HID service, including services
/// that do not expose enough public identity to support a persistent rule.
pub fn list_pointing_device_observations() -> AppResult<Vec<ObservedDevice>> {
    let mut devices: Vec<_> = mouse_device_snapshots()?
        .into_iter()
        .map(|snapshot| ObservedDevice {
            identity: snapshot.identity,
            name: snapshot.name,
            transport: snapshot.transport,
        })
        .collect();
    devices.sort_by(|left, right| {
        left.identity
            .cmp(&right.identity)
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.transport.cmp(&right.transport))
    });
    devices.dedup();
    Ok(devices)
}

fn mouse_device_snapshots() -> AppResult<Vec<MouseDeviceSnapshot>> {
    unsafe {
        let manager = IOHIDManagerCreate(ptr::null(), KIOHID_OPTIONS_TYPE_NONE);
        if manager.is_null() {
            return Err(AppError::Platform(
                "IOHIDManagerCreate returned NULL".to_string(),
            ));
        }

        let matching = mouse_matching_dictionary();
        IOHIDManagerSetDeviceMatching(manager, matching.as_concrete_TypeRef());
        let devices = mouse_device_snapshots_from_manager(manager);
        CFRelease(manager as CFTypeRef);

        Ok(devices)
    }
}

unsafe fn mouse_device_snapshots_from_manager(
    manager: IOHIDManagerRef,
) -> Vec<MouseDeviceSnapshot> {
    let device_set = unsafe { IOHIDManagerCopyDevices(manager) };
    let mut devices = Vec::new();
    if !device_set.is_null() {
        let count = unsafe { CFSetGetCount(device_set) } as usize;
        let mut refs: Vec<*const c_void> = vec![ptr::null(); count];
        unsafe { CFSetGetValues(device_set, refs.as_mut_ptr()) };
        for device in refs {
            let device = device as IOHIDDeviceRef;
            devices.push(MouseDeviceSnapshot {
                identity: unsafe { device_identity_of(device) },
                name: unsafe { string_property(device, KEY_PRODUCT) },
                transport: unsafe { string_property(device, KEY_TRANSPORT) },
            });
        }
        unsafe { CFRelease(device_set as CFTypeRef) };
    }
    devices
}

fn continuous_device_name_flags(name: &str) -> (bool, bool) {
    let normalized = name.to_ascii_lowercase();
    (
        normalized.contains("trackpad"),
        normalized.contains("magic mouse"),
    )
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

unsafe fn device_identity_of(device: IOHIDDeviceRef) -> Option<DeviceIdentity> {
    let hardware = unsafe { hardware_id_of(device)? };
    let serial_number = unsafe { string_property(device, KEY_SERIAL_NUMBER) }.map(Arc::<str>::from);
    let location_id = unsafe { number_property(device, KEY_LOCATION_ID) }
        .and_then(|value| u32::try_from(value).ok());
    Some(DeviceIdentity::new(hardware, serial_number, location_id))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connected_apple_product_names_map_to_continuous_sources() {
        assert_eq!(
            continuous_device_name_flags("Apple Internal Keyboard / Trackpad"),
            (true, false)
        );
        assert_eq!(
            continuous_device_name_flags("Magic Trackpad 2"),
            (true, false)
        );
        assert_eq!(continuous_device_name_flags("Magic Mouse"), (false, true));
    }

    #[test]
    fn ordinary_discrete_devices_do_not_create_a_continuous_hint() {
        assert_eq!(
            continuous_device_name_flags("Logitech USB Receiver"),
            (false, false)
        );
        assert_eq!(
            continuous_device_name_flags("HyperX Alloy Origins 65"),
            (false, false)
        );
    }

    #[test]
    fn incomplete_hardware_identity_does_not_hide_continuous_product_names() {
        let snapshots = [
            MouseDeviceSnapshot {
                identity: None,
                name: Some("Apple Internal Keyboard / Trackpad".to_string()),
                transport: Some("FIFO".to_string()),
            },
            MouseDeviceSnapshot {
                identity: None,
                name: Some("Magic Mouse".to_string()),
                transport: Some("Bluetooth".to_string()),
            },
        ];

        assert_eq!(
            continuous_source_hint_from_snapshots(&snapshots),
            ContinuousSourceHint::Both
        );
    }
}
