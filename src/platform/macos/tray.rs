//! Native macOS menu-bar status item for the merged settings-window +
//! event-tap process.
//!
//! This intentionally uses AppKit's `NSStatusItem` directly instead of the
//! cross-platform `tray-icon` wrapper. On the current macOS 26 dev machine,
//! `tray-icon` repeatedly asked Control Center for `NSStatusItemView` scenes
//! and got `BSServiceConnectionErrorDomain code=3`, leaving no visible menu
//! item.

use std::sync::atomic::{AtomicU8, Ordering};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSImage, NSImageNameRefreshTemplate, NSMenu, NSMenuItem, NSSquareStatusItemLength, NSStatusBar,
    NSStatusItem,
};
use objc2_foundation::NSString;

const NO_ACTION: u8 = 0;
const OPEN_SETTINGS_ACTION: u8 = 1;
const QUIT_ACTION: u8 = 2;

static PENDING_ACTION: AtomicU8 = AtomicU8::new(NO_ACTION);

/// What the user asked for via the tray menu, polled once per frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    OpenSettings,
    Quit,
}

/// Owns the live AppKit objects. Dropping this removes the menu-bar item.
pub struct TrayHandle {
    _status_item: Retained<NSStatusItem>,
    _icon: Retained<NSImage>,
    _menu: Retained<NSMenu>,
    _target: Retained<MenuActionTarget>,
}

#[derive(Debug)]
struct MenuActionTargetIvars;

define_class!(
    #[unsafe(super(NSObject))]
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
    }
);

/// Builds the menu-bar item and its menu. Must be called on the main thread
/// (eframe's thread), same as every AppKit/NSStatusItem call.
pub fn build() -> Result<TrayHandle, String> {
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| "could not create the menu-bar item off the main thread".to_string())?;

    let target_alloc = mtm.alloc().set_ivars(MenuActionTargetIvars);
    let target: Retained<MenuActionTarget> = unsafe { msg_send![super(target_alloc), init] };

    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("Auto Reverse"));
    menu.addItem(&menu_item(
        "Open Settings",
        sel!(openSettings:),
        &target,
        mtm,
    ));
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    menu.addItem(&menu_item("Quit", sel!(quit:), &target, mtm));

    let icon = menu_bar_icon()?;
    let status_item = NSStatusBar::systemStatusBar().statusItemWithLength(NSSquareStatusItemLength);
    #[allow(deprecated)]
    status_item.setImage(Some(&icon));
    status_item.setMenu(Some(&menu));
    status_item.setVisible(true);

    Ok(TrayHandle {
        _status_item: status_item,
        _icon: icon,
        _menu: menu,
        _target: target,
    })
}

/// Non-blocking poll for a tray action, meant to be called once per eframe
/// update tick.
pub fn poll_action() -> Option<TrayAction> {
    match PENDING_ACTION.swap(NO_ACTION, Ordering::SeqCst) {
        OPEN_SETTINGS_ACTION => Some(TrayAction::OpenSettings),
        QUIT_ACTION => Some(TrayAction::Quit),
        _ => None,
    }
}

fn menu_bar_icon() -> Result<Retained<NSImage>, String> {
    let icon = NSImage::imageNamed(unsafe { NSImageNameRefreshTemplate })
        .ok_or_else(|| "could not load the native Refresh template menu-bar icon".to_string())?;
    icon.setTemplate(true);
    icon.setAccessibilityDescription(Some(&NSString::from_str("Auto Reverse")));
    Ok(icon)
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
