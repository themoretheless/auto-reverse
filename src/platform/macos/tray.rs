//! Native macOS menu-bar status item for the merged settings-window +
//! event-tap process.
//!
//! This intentionally uses AppKit's `NSStatusItem` directly instead of the
//! cross-platform `tray-icon` wrapper. On the current macOS 26 dev machine,
//! `tray-icon` repeatedly asked Control Center for `NSStatusItemView` scenes
//! and got `BSServiceConnectionErrorDomain code=3`, leaving no visible menu
//! item.
//!
//! ## Icon
//!
//! The glyph is drawn in code, not loaded from a bundled asset: two
//! opposing vertical arrows (handoff "1c", concept B - "reads as
//! reverse"), built via `NSBezierPath` inside an
//! `NSImage.imageWithSize:flipped:drawingHandler:` block (bound by
//! objc2-app-kit behind the `block2` Cargo feature). That selector is the
//! documented, resolution-independent replacement for the old
//! lockFocus-style custom `NSImage` drawing, and unlike a fixed-resolution
//! bitmap it re-rasterizes cleanly on any screen backing-store scale. The
//! glyph itself is left uncolored (drawn with `NSColor::blackColor` and
//! `setTemplate(true)`, exactly like the old `NSImageNameRefreshTemplate`
//! icon it replaces) so AppKit still auto-tints it for light/dark menu
//! bars.
//!
//! The status dot is NOT baked into that same bitmap - an earlier version
//! of this module tried that (drawing the dot into the same template
//! `NSImage` as the arrows) on the theory that AppKit's template tint only
//! recolors "glyph" pixels and leaves a saturated badge color alone. That
//! is wrong: `setTemplate(true)` recolors every opaque pixel of the image
//! using only its alpha channel as a mask, with no per-region exception for
//! a differently-colored badge, so the dot would render solid black/white
//! like the arrows, not green/gray/orange - silently defeating the whole
//! point of a status indicator. The dot is instead a separate, non-template
//! `NSImageView` overlay added as a subview of the status item's own
//! `NSStatusBarButton` (see `TrayHandle::dot_view`), positioned in its
//! bottom-right corner - the same technique real menu-bar apps that
//! combine a template glyph with a colored badge (Slack, 1Password) use.
//! Because it is a separate view, not a separate region of one bitmap, it
//! is never subject to template tinting no matter what alpha/luminance the
//! arrows glyph has.
//!
//! ## Live menu updates
//!
//! `NSMenu` has no cheap "just this item changed" push API; the correct,
//! documented hook is `NSMenuDelegate::menuWillOpen:`, which AppKit calls
//! right before the menu is actually shown. The status line, the "Reverse
//! Scrolling" checkmark, and the Devices submenu's checkmarks are rebuilt
//! there - not on every `poll_action` tick - so a stale menu is impossible
//! (it is always rebuilt against the live config right before the user
//! sees it) without paying rebuild cost 4x/second while the menu is
//! closed. The status item's ICON is a separate concern: `logic()` (see
//! `ui.rs`) polls the computed `TrayStatus` once per tick and only calls
//! `TrayHandle::set_status` when it actually changed, to avoid needless
//! image churn every 250ms forever.
//!
//! Holding Option while clicking the icon is handled in `menuWillOpen:`:
//! the menu is canceled before it draws and the Debug Console action is
//! emitted instead, matching the design handoff's option-click entry point
//! while keeping "Open Debug Console..." in the rich menu as a discoverable
//! fallback.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, NSObject, NSObjectProtocol};
use objc2::{DeclaredClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAppearanceCustomization, NSBezierPath, NSColor, NSControlStateValueOff,
    NSControlStateValueOn, NSEvent, NSEventModifierFlags, NSImage, NSImageView, NSMenu,
    NSMenuDelegate, NSMenuItem, NSSquareStatusItemLength, NSStatusBar, NSStatusBarButton,
    NSStatusItem,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

use crate::config::AppConfig;
use crate::platform::macos::hid;
use crate::runtime::{DEFAULT_PAUSE_DURATION, RuntimeControl};

mod device_rules;

use device_rules::toggle_device_rules;

const NO_ACTION: usize = 0;
const OPEN_SETTINGS_ACTION: usize = 1;
const QUIT_ACTION: usize = 2;
const TOGGLE_ENABLED_ACTION: usize = 3;
const OPEN_DEBUG_CONSOLE_ACTION: usize = 4;
const SAVE_FAILED_ACTION: usize = 5;
const PAUSE_CHANGED_ACTION: usize = 6;
// Device-rule quick-pick actions are encoded as
// DEVICE_ACTION_BASE + index into the device list snapshotted when the
// menu was last built (see `MenuActionTargetIvars::devices`), rather than
// one action constant per device, since the device count is dynamic.
const DEVICE_ACTION_BASE: usize = 100;

static PENDING_ACTION: AtomicUsize = AtomicUsize::new(NO_ACTION);
static LAST_SAVE_ERROR: Mutex<Option<String>> = Mutex::new(None);

type SaveCallback = dyn Fn(&AppConfig) -> Result<(), String> + Send + Sync;

/// What the user asked for via the tray menu, polled once per frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    OpenSettings,
    Quit,
    /// Toggle `config.enabled` directly from the tray's "Reverse Scrolling"
    /// item.
    ToggleEnabled,
    /// Set/clear the device rule for the device at this index in the
    /// snapshot the menu was last built with (see `MenuActionTargetIvars`).
    ToggleDevice(usize),
    /// Open the Debug Console window from the rich menu or an option-click.
    OpenDebugConsole,
    /// A tray-originated config mutation could not be persisted and was
    /// rolled back in shared config.
    SaveFailed,
    /// Temporary process-local pause started or ended.
    PauseChanged,
}

/// Status the icon and the menu's status line both reflect. Computed the
/// same way `ui.rs`'s `status_header` computes it, so the tray can never
/// silently disagree with the settings window about whether Auto Reverse
/// is "on".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayStatus {
    Active,
    Paused,
    TemporarilyPaused,
    NeedsPermission,
}

impl TrayStatus {
    pub fn from_config(
        config: &AppConfig,
        permissions_ready: bool,
        temporarily_paused: bool,
    ) -> Self {
        Self::from_state(config.enabled, permissions_ready, temporarily_paused)
    }

    fn from_state(enabled: bool, permissions_ready: bool, temporarily_paused: bool) -> Self {
        if !enabled {
            Self::Paused
        } else if !permissions_ready {
            Self::NeedsPermission
        } else if temporarily_paused {
            Self::TemporarilyPaused
        } else {
            Self::Active
        }
    }

    /// Dot color for this status, matching the handoff's "Concept B - every
    /// state" section exactly - which specifies a DIFFERENT (if close) tone
    /// per menu-bar appearance, not one fixed color: light menu bar reads
    /// `#34A853`/`#808080`/`#E59E2F`, dark menu bar reads
    /// `#34C759`/`#8E8E93`/`#FF9F0A`. An earlier version of this function
    /// always used the light-bar values regardless of the actual menu bar
    /// appearance - a real, if subtle, deviation from the design.
    fn dot_rgb(self, dark: bool) -> (f64, f64, f64) {
        match (self, dark) {
            (Self::Active, false) => (
                0x34 as f64 / 255.0,
                0xA8 as f64 / 255.0,
                0x53 as f64 / 255.0,
            ),
            (Self::Active, true) => (
                0x34 as f64 / 255.0,
                0xC7 as f64 / 255.0,
                0x59 as f64 / 255.0,
            ),
            (Self::Paused, false) => (
                0x80 as f64 / 255.0,
                0x80 as f64 / 255.0,
                0x80 as f64 / 255.0,
            ),
            (Self::Paused, true) => (
                0x8E as f64 / 255.0,
                0x8E as f64 / 255.0,
                0x93 as f64 / 255.0,
            ),
            (Self::TemporarilyPaused, false) => (
                0x80 as f64 / 255.0,
                0x80 as f64 / 255.0,
                0x80 as f64 / 255.0,
            ),
            (Self::TemporarilyPaused, true) => (
                0x8E as f64 / 255.0,
                0x8E as f64 / 255.0,
                0x93 as f64 / 255.0,
            ),
            (Self::NeedsPermission, false) => (
                0xE5 as f64 / 255.0,
                0x9E as f64 / 255.0,
                0x2F as f64 / 255.0,
            ),
            (Self::NeedsPermission, true) => (
                0xFF as f64 / 255.0,
                0x9F as f64 / 255.0,
                0x0A as f64 / 255.0,
            ),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Active => "On",
            Self::Paused => "Off",
            Self::TemporarilyPaused => "Paused temporarily",
            Self::NeedsPermission => "Needs permission",
        }
    }
}

/// Owns the live AppKit objects. Dropping this removes the menu-bar item.
pub struct TrayHandle {
    /// Kept alive so dropping `TrayHandle` removes the menu-bar item. Its
    /// visibility follows `show_menu_bar_icon`; the menu and image remain
    /// attached while hidden so restoring it is immediate and lossless.
    status_item: Retained<NSStatusItem>,
    /// The status item's button, kept so `set_status` can re-check its
    /// `effectiveAppearance` on every status change - the menu bar's
    /// light/dark state can change independently of the app's own windows
    /// (e.g. "Automatic" appearance following wallpaper), so this cannot be
    /// determined once at `build()` time and cached.
    button: Option<Retained<NSStatusBarButton>>,
    /// Non-template overlay showing the current status color, added as a
    /// subview of the status item's button - see the module doc comment for
    /// why the dot cannot live inside the template-tinted glyph image.
    /// `None` only if AppKit did not hand back a button to attach it to
    /// (`NSStatusItem::button` is `Option` in the API; not expected to be
    /// `None` in practice for a status item created with a square length).
    dot_view: Option<Retained<NSImageView>>,
    /// Kept alive only so dropping `TrayHandle` tears down the menu (and,
    /// via `target`'s ivars, releases the shared config `Arc`) - not read
    /// again after `build()` sets it as the status item's menu.
    _menu: Retained<NSMenu>,
    /// Kept alive for the same reason as `_menu` - AppKit does not retain
    /// the delegate/target strongly enough to outlive this handle.
    _target: Retained<MenuActionTarget>,
    /// Last status the dot was actually redrawn for, so `set_status` can
    /// skip regenerating its image when nothing changed.
    last_icon_status: Option<TrayStatus>,
    /// Last menu-bar appearance bucket used to draw the dot. This is not
    /// derivable from `last_icon_status`: macOS can switch the menu bar
    /// between light and dark while Auto Reverse stays Active/Paused, and
    /// the handoff defines different dot colors for those appearances.
    last_icon_dark: Option<bool>,
    /// Last value sent to AppKit, avoiding a redundant Objective-C message on
    /// every 250 ms logic tick.
    visible: bool,
}

impl TrayHandle {
    /// Redraws only the small dot overlay if `status` differs from the last
    /// one actually drawn - the arrows glyph itself never changes and is
    /// never touched again after `build()`. Called once per `logic()` tick
    /// from `ui.rs` with a cheap `TrayStatus::from_config` - the drawing-
    /// handler path only runs on an actual change, not on every tick.
    pub fn set_status(&mut self, status: TrayStatus) {
        let dark = self.button.as_deref().is_some_and(is_dark_menu_bar);
        if self.last_icon_status == Some(status) && self.last_icon_dark == Some(dark) {
            return;
        }
        if let Some(dot_view) = &self.dot_view {
            dot_view.setImage(Some(&dot_image(status, dark)));
        }
        self.last_icon_status = Some(status);
        self.last_icon_dark = Some(dark);
    }

    /// Hides or restores the existing status item without rebuilding its
    /// target, menu, device snapshot, or status-dot views.
    pub fn set_visible(&mut self, visible: bool) {
        if self.visible == visible {
            return;
        }
        self.status_item.setVisible(visible);
        self.visible = visible;
    }
}

/// Ivars for the single Objective-C target object that backs both the
/// menu-item actions and the `NSMenuDelegate` live-refresh hook. Holds the
/// shared config plus a snapshot of the device list taken the last time the
/// menu was rebuilt, so `ToggleDevice(index)` (fired from a click, which
/// only carries the sender, not our Rust index) can be resolved back to a
/// concrete `DeviceIdentity` in `logic()`.
struct MenuActionTargetIvars {
    shared_config: Arc<RwLock<AppConfig>>,
    runtime_control: Arc<RuntimeControl>,
    permissions_ready: Arc<AtomicBool>,
    on_change: Arc<SaveCallback>,
    device_snapshot: Mutex<Vec<hid::DeviceInfo>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "AutoReverseMenuActionTarget"]
    #[ivars = MenuActionTargetIvars]
    struct MenuActionTarget;

    impl MenuActionTarget {
        #[unsafe(method(openSettings:))]
        fn open_settings(&self, _sender: &AnyObject) {
            PENDING_ACTION.store(OPEN_SETTINGS_ACTION, Ordering::SeqCst);
        }

        #[unsafe(method(quit:))]
        fn quit(&self, _sender: &AnyObject) {
            PENDING_ACTION.store(QUIT_ACTION, Ordering::SeqCst);
        }

        #[unsafe(method(toggleEnabled:))]
        fn toggle_enabled(&self, _sender: &AnyObject) {
            let ivars = self.ivars();
            let (old_value, new_value) = {
                let mut guard = match ivars.shared_config.write() {
                    Ok(guard) => guard,
                    Err(poisoned) => poisoned.into_inner(),
                };
                let old_value = guard.clone();
                guard.enabled = !guard.enabled;
                (old_value, guard.clone())
            };
            match (ivars.on_change)(&new_value) {
                Ok(()) => {
                    PENDING_ACTION.store(TOGGLE_ENABLED_ACTION, Ordering::SeqCst);
                }
                Err(error) => {
                    restore_shared_config(&ivars.shared_config, old_value);
                    remember_save_error(error);
                    PENDING_ACTION.store(SAVE_FAILED_ACTION, Ordering::SeqCst);
                }
            }
        }

        #[unsafe(method(openDebugConsole:))]
        fn open_debug_console(&self, _sender: &AnyObject) {
            PENDING_ACTION.store(OPEN_DEBUG_CONSOLE_ACTION, Ordering::SeqCst);
        }

        #[unsafe(method(pauseTemporarily:))]
        fn pause_temporarily(&self, _sender: &AnyObject) {
            self.ivars()
                .runtime_control
                .pause_for(DEFAULT_PAUSE_DURATION);
            PENDING_ACTION.store(PAUSE_CHANGED_ACTION, Ordering::SeqCst);
        }

        #[unsafe(method(resumeNow:))]
        fn resume_now(&self, _sender: &AnyObject) {
            self.ivars().runtime_control.resume();
            PENDING_ACTION.store(PAUSE_CHANGED_ACTION, Ordering::SeqCst);
        }

        #[unsafe(method(toggleDevice:))]
        fn toggle_device(&self, sender: &AnyObject) {
            let tag: isize = unsafe { msg_send![sender, tag] };
            let index = (tag - DEVICE_ACTION_BASE as isize) as usize;
            let ivars = self.ivars();
            let device = {
                let snapshot = ivars.device_snapshot.lock().unwrap_or_else(|p| p.into_inner());
                snapshot.get(index).cloned()
            };
            let Some(device) = device else { return };

            let (old_value, new_value) = {
                let mut guard = match ivars.shared_config.write() {
                    Ok(guard) => guard,
                    Err(poisoned) => poisoned.into_inner(),
                };
                let old_value = guard.clone();
                let Some(updated_rules) = toggle_device_rules(&guard.device_rules, &device) else {
                    // An explicit "Don't reverse" rule (Some(false)) is a
                    // deliberate choice made via the settings window's
                    // three-way Default/Reverse/Don't-reverse control - the
                    // tray's quick-pick only has a binary checkmark and
                    // cannot represent that third state, so it must never
                    // silently overwrite it with Reverse. `rebuild_menu`
                    // already disables this menu item for that case; this
                    // is a second, defensive guard against ever mutating it
                    // from here (see `toggle_device_rules`'s own tests).
                    return;
                };
                guard.device_rules = updated_rules;
                (old_value, guard.clone())
            };
            match (ivars.on_change)(&new_value) {
                Ok(()) => {
                    // Reuses the same DEVICE_ACTION_BASE+index encoding as the menu
                    // item's own tag (see rebuild_menu) so poll_action can decode it
                    // straight back into TrayAction::ToggleDevice(index) - the
                    // settings window's self.config resync (see ui.rs's logic())
                    // needs this action, not NO_ACTION, to know a tray-driven device
                    // toggle just happened.
                    PENDING_ACTION.store(encode_device_action(index), Ordering::SeqCst);
                }
                Err(error) => {
                    restore_shared_config(&ivars.shared_config, old_value);
                    remember_save_error(error);
                    PENDING_ACTION.store(SAVE_FAILED_ACTION, Ordering::SeqCst);
                }
            }
        }
    }

    unsafe impl NSObjectProtocol for MenuActionTarget {}

    unsafe impl NSMenuDelegate for MenuActionTarget {
        #[allow(non_snake_case)] // matches the ObjC selector menuWillOpen:
        #[unsafe(method(menuWillOpen:))]
        fn menuWillOpen(&self, menu: &NSMenu) {
            if NSEvent::modifierFlags_class().contains(NSEventModifierFlags::Option) {
                PENDING_ACTION.store(OPEN_DEBUG_CONSOLE_ACTION, Ordering::SeqCst);
                menu.cancelTrackingWithoutAnimation();
                return;
            }

            let ivars = self.ivars();
            let config = {
                let guard = match ivars.shared_config.read() {
                    Ok(guard) => guard,
                    Err(poisoned) => poisoned.into_inner(),
                };
                guard.clone()
            };
            let devices = hid::list_pointing_devices().unwrap_or_default();
            {
                let mut snapshot = ivars.device_snapshot.lock().unwrap_or_else(|p| p.into_inner());
                *snapshot = devices.clone();
            }
            let mtm = self.mtm();
            rebuild_menu(
                menu,
                self,
                &config,
                &devices,
                &ivars.runtime_control,
                ivars.permissions_ready.load(Ordering::Acquire),
                mtm,
            );
        }
    }
);

/// Builds the menu-bar item and its menu. Must be called on the main thread
/// (eframe's thread), same as every AppKit/NSStatusItem call.
///
/// `shared_config` is the SAME `Arc<RwLock<AppConfig>>` the settings window
/// and the background tap thread already share (see `ui.rs`'s module doc
/// comment) - toggling from the tray writes through it and calls
/// `on_disk_save`, so there is exactly one write path to disk, not a second
/// one that could silently diverge from `SettingsApp::save()`.
pub fn build(
    shared_config: Arc<RwLock<AppConfig>>,
    runtime_control: Arc<RuntimeControl>,
    permissions_ready: Arc<AtomicBool>,
    on_disk_save: impl Fn(&AppConfig) -> Result<(), String> + Send + Sync + 'static,
) -> Result<TrayHandle, String> {
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| "could not create the menu-bar item off the main thread".to_string())?;

    let config_snapshot = {
        let guard = match shared_config.read() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.clone()
    };
    let devices = hid::list_pointing_devices().unwrap_or_default();

    let target_alloc = mtm.alloc().set_ivars(MenuActionTargetIvars {
        shared_config,
        runtime_control: Arc::clone(&runtime_control),
        permissions_ready: Arc::clone(&permissions_ready),
        on_change: Arc::new(on_disk_save),
        device_snapshot: Mutex::new(devices.clone()),
    });
    let target: Retained<MenuActionTarget> = unsafe { msg_send![super(target_alloc), init] };

    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("Auto Reverse"));
    menu.setDelegate(Some(objc2::runtime::ProtocolObject::from_ref(&*target)));
    rebuild_menu(
        &menu,
        &target,
        &config_snapshot,
        &devices,
        &runtime_control,
        permissions_ready.load(Ordering::Acquire),
        mtm,
    );

    let status = TrayStatus::from_config(
        &config_snapshot,
        permissions_ready.load(Ordering::Acquire),
        runtime_control.is_paused(),
    );
    let icon = arrows_icon()?;
    let status_item = NSStatusBar::systemStatusBar().statusItemWithLength(NSSquareStatusItemLength);
    #[allow(deprecated)]
    status_item.setImage(Some(&icon));
    status_item.setMenu(Some(&menu));
    status_item.setVisible(config_snapshot.show_menu_bar_icon);

    // Status dot: a separate, non-template `NSImageView` subview of the
    // status item's button, positioned in its bottom-right corner - see the
    // module doc comment for why this must not be part of the arrows'
    // template `NSImage`. `button(mtm)` can in principle return `None`
    // (the API is `Option`); if so, the status item still works, just
    // without the colored dot, rather than failing the whole tray build.
    let button = status_item.button(mtm);
    let dot_view = button.as_ref().map(|button| {
        let bounds = button.bounds();
        const DOT_DIAMETER: f64 = 6.0;
        const MARGIN: f64 = 1.0;
        let frame = NSRect::new(
            NSPoint::new(bounds.size.width - DOT_DIAMETER - MARGIN, MARGIN),
            NSSize::new(DOT_DIAMETER, DOT_DIAMETER),
        );
        let view = NSImageView::initWithFrame(NSImageView::alloc(mtm), frame);
        view.setImage(Some(&dot_image(status, is_dark_menu_bar(button))));
        // Default autoresizing mask (untouched, i.e. NSViewNotSizable) is
        // exactly "stay put" - correct here since the button's own size is
        // effectively fixed for the process's lifetime.
        button.addSubview(&view);
        view
    });

    let initial_icon_dark = button.as_deref().map(is_dark_menu_bar);

    Ok(TrayHandle {
        status_item,
        button,
        dot_view,
        _menu: menu,
        _target: target,
        last_icon_status: Some(status),
        last_icon_dark: initial_icon_dark,
        visible: config_snapshot.show_menu_bar_icon,
    })
}

/// Rebuilds every item in `menu` from scratch against the current config and
/// device list. Called both when the menu is first built and every time
/// `menuWillOpen:` fires (see the module doc comment for why a full rebuild,
/// rather than in-place mutation, is the simplest correct approach here).
fn rebuild_menu(
    menu: &NSMenu,
    target: &MenuActionTarget,
    config: &AppConfig,
    devices: &[hid::DeviceInfo],
    runtime_control: &RuntimeControl,
    permissions_ready: bool,
    mtm: MainThreadMarker,
) {
    menu.removeAllItems();

    let temporarily_paused = runtime_control.is_paused();
    let status = TrayStatus::from_config(config, permissions_ready, temporarily_paused);
    let status_item = NSMenuItem::new(mtm);
    status_item.setTitle(&NSString::from_str(&format!(
        "Auto Reverse — {}",
        status.label()
    )));
    status_item.setEnabled(false);
    menu.addItem(&status_item);

    menu.addItem(&NSMenuItem::separatorItem(mtm));

    let reverse_item = menu_item("Reverse Scrolling", sel!(toggleEnabled:), target, mtm);
    reverse_item.setState(if config.enabled {
        NSControlStateValueOn
    } else {
        NSControlStateValueOff
    });
    menu.addItem(&reverse_item);

    let (pause_title, pause_selector) = if temporarily_paused {
        ("Resume Now", sel!(resumeNow:))
    } else {
        ("Pause for 15 Minutes", sel!(pauseTemporarily:))
    };
    let pause_item = menu_item(pause_title, pause_selector, target, mtm);
    if !temporarily_paused && status != TrayStatus::Active {
        pause_item.setEnabled(false);
    }
    menu.addItem(&pause_item);

    let devices_item = NSMenuItem::new(mtm);
    devices_item.setTitle(&NSString::from_str("Devices"));
    if devices.is_empty() {
        devices_item.setEnabled(false);
    } else {
        let submenu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("Devices"));
        for (index, device) in devices.iter().enumerate() {
            let mut label = config
                .device_alias(&device.identity)
                .map(str::to_owned)
                .or_else(|| device.name.clone())
                .unwrap_or_else(|| "Unnamed device".to_string());
            if let Some(qualifier) = device.identity.compact_qualifier() {
                label.push_str(" · ");
                label.push_str(&qualifier);
            }
            let current_rule = config
                .preferred_device_rule(&device.identity)
                .and_then(|rule| rule.reverse);
            // The tray's quick-pick only has a binary checkmark, but
            // device_rules is really three-way (Default/Reverse/Don't
            // reverse). An explicit Don't-reverse rule looks identical to
            // Default here (no checkmark either way) - disabling the item
            // and naming the reason in its title prevents a click from
            // silently overwriting that explicit choice with Reverse (see
            // toggle_device's matching defensive check) while still making
            // clear where to actually change it.
            let title = if current_rule == Some(false) {
                format!("{label} (Don't reverse — see Settings)")
            } else {
                label
            };
            let item = menu_item(&title, sel!(toggleDevice:), target, mtm);
            item.setState(if current_rule == Some(true) {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });
            item.setTag(device_action_tag(index));
            if current_rule == Some(false) {
                item.setEnabled(false);
            }
            submenu.addItem(&item);
        }
        devices_item.setSubmenu(Some(&submenu));
    }
    menu.addItem(&devices_item);

    menu.addItem(&NSMenuItem::separatorItem(mtm));
    menu.addItem(&menu_item(
        "Open Settings…",
        sel!(openSettings:),
        target,
        mtm,
    ));
    menu.addItem(&menu_item(
        "Open Debug Console…",
        sel!(openDebugConsole:),
        target,
        mtm,
    ));
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    menu.addItem(&menu_item("Quit Auto Reverse", sel!(quit:), target, mtm));
}

/// Non-blocking poll for a tray action, meant to be called once per eframe
/// update tick.
pub fn poll_action() -> Option<TrayAction> {
    decode_pending_action(PENDING_ACTION.swap(NO_ACTION, Ordering::SeqCst))
}

fn encode_device_action(index: usize) -> usize {
    DEVICE_ACTION_BASE + index
}

fn device_action_tag(index: usize) -> isize {
    encode_device_action(index).min(isize::MAX as usize) as isize
}

fn decode_pending_action(raw: usize) -> Option<TrayAction> {
    match raw {
        OPEN_SETTINGS_ACTION => Some(TrayAction::OpenSettings),
        QUIT_ACTION => Some(TrayAction::Quit),
        TOGGLE_ENABLED_ACTION => Some(TrayAction::ToggleEnabled),
        OPEN_DEBUG_CONSOLE_ACTION => Some(TrayAction::OpenDebugConsole),
        SAVE_FAILED_ACTION => Some(TrayAction::SaveFailed),
        PAUSE_CHANGED_ACTION => Some(TrayAction::PauseChanged),
        NO_ACTION => None,
        // Anything at/above DEVICE_ACTION_BASE is a device-toggle index -
        // see toggle_device's PENDING_ACTION.store call for the encoding.
        raw if raw >= DEVICE_ACTION_BASE => {
            Some(TrayAction::ToggleDevice(raw - DEVICE_ACTION_BASE))
        }
        _ => None,
    }
}

pub fn take_last_save_error() -> Option<String> {
    let mut guard = LAST_SAVE_ERROR.lock().unwrap_or_else(|p| p.into_inner());
    guard.take()
}

fn remember_save_error(error: String) {
    let mut guard = LAST_SAVE_ERROR.lock().unwrap_or_else(|p| p.into_inner());
    *guard = Some(error);
}

fn restore_shared_config(shared_config: &RwLock<AppConfig>, old_value: AppConfig) {
    let mut guard = match shared_config.write() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    *guard = old_value;
}

/// Draws only the "opposing arrows" glyph (handoff "1c", concept B) into a
/// template `NSImage`, sized for an 18x18 menu-bar icon (the same nominal
/// size the old `NSImageNameRefreshTemplate` icon rendered at). Contains no
/// status dot - built once in `build()` and never regenerated, since the
/// glyph itself never changes; only the separate dot overlay does (see the
/// module doc comment and `dot_image`).
fn arrows_icon() -> Result<Retained<NSImage>, String> {
    let size = NSSize::new(18.0, 18.0);

    // AppKit invokes this block synchronously, on the calling (main)
    // thread, from inside `imageWithSize:flipped:drawingHandler:` itself -
    // never stored or invoked later/concurrently.
    let handler = RcBlock::new(move |_rect: NSRect| -> Bool {
        draw_arrows();
        Bool::YES
    });

    let icon = NSImage::imageWithSize_flipped_drawingHandler(size, true, &handler);
    icon.setTemplate(true);
    icon.setAccessibilityDescription(Some(&NSString::from_str("Auto Reverse")));
    Ok(icon)
}

/// The actual NSBezierPath drawing for the arrows glyph, split out of
/// `arrows_icon` for clarity. Coordinate system: flipped (origin top-left,
/// y grows down), mirroring the handoff's SVG `viewBox="0 24 24"`
/// coordinates directly - the path data below is the exact one from the
/// handoff's id="1c" Concept B, scaled from its 24x24 viewBox down to the
/// 18x18 icon (scale factor 18/24 = 0.75).
fn draw_arrows() {
    const SCALE: f64 = 18.0 / 24.0;

    NSColor::blackColor().set();
    let arrows = NSBezierPath::bezierPath();
    arrows.setLineWidth(2.0 * SCALE);
    arrows.setLineCapStyle(objc2_app_kit::NSLineCapStyle::Round);
    arrows.setLineJoinStyle(objc2_app_kit::NSLineJoinStyle::Round);

    // Left arrow: M8.5 19V5, M8.5 5L5.5 8, M8.5 5L11.5 8 (points up).
    arrows.moveToPoint(NSPoint::new(8.5 * SCALE, 19.0 * SCALE));
    arrows.lineToPoint(NSPoint::new(8.5 * SCALE, 5.0 * SCALE));
    arrows.moveToPoint(NSPoint::new(8.5 * SCALE, 5.0 * SCALE));
    arrows.lineToPoint(NSPoint::new(5.5 * SCALE, 8.0 * SCALE));
    arrows.moveToPoint(NSPoint::new(8.5 * SCALE, 5.0 * SCALE));
    arrows.lineToPoint(NSPoint::new(11.5 * SCALE, 8.0 * SCALE));

    // Right arrow: M15.5 5V19, M15.5 19L12.5 16, M15.5 19L18.5 16 (points down).
    arrows.moveToPoint(NSPoint::new(15.5 * SCALE, 5.0 * SCALE));
    arrows.lineToPoint(NSPoint::new(15.5 * SCALE, 19.0 * SCALE));
    arrows.moveToPoint(NSPoint::new(15.5 * SCALE, 19.0 * SCALE));
    arrows.lineToPoint(NSPoint::new(12.5 * SCALE, 16.0 * SCALE));
    arrows.moveToPoint(NSPoint::new(15.5 * SCALE, 19.0 * SCALE));
    arrows.lineToPoint(NSPoint::new(18.5 * SCALE, 16.0 * SCALE));

    arrows.stroke();
}

/// True when the status item's button is currently drawn on a dark menu
/// bar. Checked via `effectiveAppearance` (not the app's own window
/// appearance) because the menu bar can be dark while app windows are
/// light or vice versa (e.g. "Automatic" appearance following wallpaper) -
/// `bestMatch` isn't used here since a plain substring check on the
/// appearance name already buckets the accessibility high-contrast dark
/// variant correctly (its name also contains "Dark"), and the two dot
/// palettes are close enough that exact protocol compliance isn't needed.
fn is_dark_menu_bar(button: &NSStatusBarButton) -> bool {
    button
        .effectiveAppearance()
        .name()
        .to_string()
        .contains("Dark")
}

/// Draws the small status-dot overlay image: a solid-color filled circle,
/// deliberately NEVER marked template - its color must survive exactly as
/// given (green/gray/orange), unlike the arrows glyph above. Used as the
/// image for `TrayHandle::dot_view`, a separate `NSImageView` subview, not
/// composited into `arrows_icon`'s bitmap - see the module doc comment for
/// why. `dark` selects the handoff's dark-menu-bar dot palette
/// (`#34C759`/`#8E8E93`/`#FF9F0A`) instead of the light one.
fn dot_image(status: TrayStatus, dark: bool) -> Retained<NSImage> {
    let (dot_r, dot_g, dot_b) = status.dot_rgb(dark);
    const DOT_DIAMETER: f64 = 6.0;
    let size = NSSize::new(DOT_DIAMETER, DOT_DIAMETER);

    let handler = RcBlock::new(move |_rect: NSRect| -> Bool {
        let dot_color = NSColor::colorWithSRGBRed_green_blue_alpha(dot_r, dot_g, dot_b, 1.0);
        dot_color.set();
        let dot_path = NSBezierPath::bezierPathWithOvalInRect(NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(DOT_DIAMETER, DOT_DIAMETER),
        ));
        dot_path.fill();
        Bool::YES
    });

    NSImage::imageWithSize_flipped_drawingHandler(size, true, &handler)
}

fn menu_item(
    title: &str,
    action: objc2::runtime::Sel,
    target: &MenuActionTarget,
    mtm: MainThreadMarker,
) -> Retained<NSMenuItem> {
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            Some(action),
            &NSString::from_str(""),
        )
    };
    unsafe {
        item.setTarget(Some(target));
    }
    item
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_action_encoding_does_not_truncate_large_indexes() {
        let index = 260;

        assert_eq!(
            decode_pending_action(encode_device_action(index)),
            Some(TrayAction::ToggleDevice(index))
        );
    }

    #[test]
    fn disabled_and_missing_permissions_outrank_temporary_pause() {
        assert_eq!(
            TrayStatus::from_state(false, false, true),
            TrayStatus::Paused
        );
        assert_eq!(
            TrayStatus::from_state(true, false, true),
            TrayStatus::NeedsPermission
        );
        assert_eq!(
            TrayStatus::from_state(true, true, true),
            TrayStatus::TemporarilyPaused
        );
    }
}
