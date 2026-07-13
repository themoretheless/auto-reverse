use std::fmt;
use std::sync::Arc;

use crate::device::{DeviceIdentity, DeviceKind};
use crate::device_source::HidSourceClass;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrollEvent {
    pub device_kind: DeviceKind,
    pub delta_vertical: i64,
    pub delta_horizontal: i64,
    pub continuous: bool,
    pub synthetic: bool,
    pub hid_source: HidSourceClass,
    /// The owning process id CGEvent reported for this event
    /// (`kCGEventSourceUnixProcessID`). A genuine hardware scroll observed
    /// through the event tap reports 0; a nonzero value means some other
    /// process posted/injected this event (e.g. a remote-desktop tool or an
    /// automation script), which `reverse_only_raw_input` can opt out of.
    pub source_pid: i64,
    /// Which specific physical device produced this scroll, when the HID
    /// wheel monitor could attribute it (discrete mouse wheels only - the
    /// CGEvent itself carries no device identity). None means "unknown
    /// device", and per-device rules simply don't apply. The Arc keeps
    /// serial/location strings allocation-free across wheel events.
    pub identity: Option<Arc<DeviceIdentity>>,
}

impl ScrollEvent {
    pub fn new(
        device_kind: DeviceKind,
        delta_vertical: i64,
        delta_horizontal: i64,
        continuous: bool,
    ) -> Self {
        Self {
            device_kind,
            delta_vertical,
            delta_horizontal,
            continuous,
            synthetic: false,
            hid_source: HidSourceClass::NotObserved,
            source_pid: 0,
            identity: None,
        }
    }
}

impl fmt::Display for ScrollEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} scroll, vertical={} horizontal={}{}",
            self.device_kind,
            self.delta_vertical,
            self.delta_horizontal,
            if self.continuous { ", continuous" } else { "" }
        )?;
        if let Some(identity) = &self.identity {
            write!(f, " [{identity}]")?;
        }
        match self.hid_source {
            HidSourceClass::Virtual => f.write_str(" [virtual HID]")?,
            HidSourceClass::Unknown => f.write_str(" [unknown HID transport]")?,
            HidSourceClass::NotObserved | HidSourceClass::Physical => {}
        }
        Ok(())
    }
}
