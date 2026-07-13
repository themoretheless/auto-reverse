//! Pure input-provenance and bypass policy shared by transform and diagnostics.

use crate::device_source::HidSourceClass;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputProvenance {
    Hardware,
    PostedProcess,
    SelfSynthetic,
    VirtualHid,
    UnknownHid,
}

impl InputProvenance {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Hardware => "hardware",
            Self::PostedProcess => "posted_process",
            Self::SelfSynthetic => "self_synthetic",
            Self::VirtualHid => "virtual_hid",
            Self::UnknownHid => "unknown_hid",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Hardware => "Hardware",
            Self::PostedProcess => "Posted/injected process",
            Self::SelfSynthetic => "Auto Reverse synthetic",
            Self::VirtualHid => "Virtual HID",
            Self::UnknownHid => "Unknown HID transport",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputBypassReason {
    SelfSynthetic,
    VirtualHid,
    UnknownHid,
    PostedInputGuard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputPolicyDecision {
    pub provenance: InputProvenance,
    pub bypass: Option<InputBypassReason>,
}

pub const fn evaluate_input_policy(
    synthetic: bool,
    hid_source: HidSourceClass,
    source_pid: i64,
    ignore_posted_input: bool,
) -> InputPolicyDecision {
    let provenance = if synthetic {
        InputProvenance::SelfSynthetic
    } else {
        match hid_source {
            HidSourceClass::Virtual => InputProvenance::VirtualHid,
            HidSourceClass::Unknown => InputProvenance::UnknownHid,
            HidSourceClass::NotObserved | HidSourceClass::Physical if source_pid != 0 => {
                InputProvenance::PostedProcess
            }
            HidSourceClass::NotObserved | HidSourceClass::Physical => InputProvenance::Hardware,
        }
    };
    let bypass = match provenance {
        InputProvenance::SelfSynthetic => Some(InputBypassReason::SelfSynthetic),
        InputProvenance::VirtualHid => Some(InputBypassReason::VirtualHid),
        InputProvenance::UnknownHid => Some(InputBypassReason::UnknownHid),
        InputProvenance::PostedProcess if ignore_posted_input => {
            Some(InputBypassReason::PostedInputGuard)
        }
        InputProvenance::Hardware | InputProvenance::PostedProcess => None,
    };
    InputPolicyDecision { provenance, bypass }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthetic_virtual_and_unknown_sources_always_bypass() {
        let cases = [
            (
                evaluate_input_policy(true, HidSourceClass::Physical, 0, false),
                InputProvenance::SelfSynthetic,
                InputBypassReason::SelfSynthetic,
            ),
            (
                evaluate_input_policy(false, HidSourceClass::Virtual, 0, false),
                InputProvenance::VirtualHid,
                InputBypassReason::VirtualHid,
            ),
            (
                evaluate_input_policy(false, HidSourceClass::Unknown, 0, false),
                InputProvenance::UnknownHid,
                InputBypassReason::UnknownHid,
            ),
        ];
        for (decision, provenance, reason) in cases {
            assert_eq!(decision.provenance, provenance);
            assert_eq!(decision.bypass, Some(reason));
        }
    }

    #[test]
    fn posted_input_has_an_explicit_opt_in_bypass() {
        let allowed = evaluate_input_policy(false, HidSourceClass::NotObserved, 42, false);
        let ignored = evaluate_input_policy(false, HidSourceClass::NotObserved, 42, true);

        assert_eq!(allowed.provenance, InputProvenance::PostedProcess);
        assert_eq!(allowed.bypass, None);
        assert_eq!(ignored.bypass, Some(InputBypassReason::PostedInputGuard));
    }

    #[test]
    fn genuine_hardware_is_processed_and_synthetic_precedence_is_stable() {
        let hardware = evaluate_input_policy(false, HidSourceClass::Physical, 0, true);
        let synthetic_virtual = evaluate_input_policy(true, HidSourceClass::Virtual, 55, true);

        assert_eq!(hardware.provenance, InputProvenance::Hardware);
        assert_eq!(hardware.bypass, None);
        assert_eq!(synthetic_virtual.provenance, InputProvenance::SelfSynthetic);
        assert_eq!(
            synthetic_virtual.bypass,
            Some(InputBypassReason::SelfSynthetic)
        );
    }
}
