use crate::device::DeviceKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollEvent {
    pub device_kind: DeviceKind,
    pub delta_vertical: i64,
    pub delta_horizontal: i64,
    pub continuous: bool,
    pub synthetic: bool,
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
        }
    }
}
