//! Pure classification of public IOHID transport values.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HidSourceClass {
    /// No HID source observation was available for this event. Existing
    /// device-kind policy remains authoritative.
    #[default]
    NotObserved,
    Physical,
    Virtual,
    Unknown,
}

impl HidSourceClass {
    pub const fn code(self) -> &'static str {
        match self {
            Self::NotObserved => "not_observed",
            Self::Physical => "physical",
            Self::Virtual => "virtual",
            Self::Unknown => "unknown",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::NotObserved => "Not observed",
            Self::Physical => "Physical HID",
            Self::Virtual => "Virtual HID",
            Self::Unknown => "Unknown HID transport",
        }
    }

    pub fn from_observed_transport(transport: Option<&str>) -> Self {
        match transport {
            Some("Virtual") => Self::Virtual,
            Some(
                "USB" | "Bluetooth" | "BluetoothLowEnergy" | "AID" | "I2C" | "SPI" | "Serial"
                | "iAP" | "BT-AACP" | "FIFO" | "SPU" | "Inductive In-Band",
            ) => Self::Physical,
            Some(_) | None => Self::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_public_virtual_transport_is_never_treated_as_physical() {
        assert_eq!(
            HidSourceClass::from_observed_transport(Some("Virtual")),
            HidSourceClass::Virtual
        );
    }

    #[test]
    fn documented_physical_transports_remain_processable() {
        for transport in ["USB", "Bluetooth", "BluetoothLowEnergy", "FIFO", "SPI"] {
            assert_eq!(
                HidSourceClass::from_observed_transport(Some(transport)),
                HidSourceClass::Physical
            );
        }
    }

    #[test]
    fn an_observed_but_unrecognized_transport_fails_open() {
        assert_eq!(
            HidSourceClass::from_observed_transport(Some("FutureTransport")),
            HidSourceClass::Unknown
        );
        assert_eq!(
            HidSourceClass::from_observed_transport(None),
            HidSourceClass::Unknown
        );
    }
}
