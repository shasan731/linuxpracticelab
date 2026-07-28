//! Hardware acceleration detection.
//!
//! Acceleration is strictly optional (spec 3.4). We never ask the learner to turn a Windows
//! feature on, never fail startup because WHPX is missing, and never require administrator
//! rights to find out. So instead of shelling out to DISM or reading the registry, we ask
//! the Windows Hypervisor Platform itself whether a hypervisor is present — a read-only,
//! unprivileged call that returns cleanly when the feature is not installed.

use shared_types::AccelMode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccelDecision {
    pub mode: AccelMode,
    /// Shown in Settings and the Environment panel so a slow lab is explainable.
    pub reason: String,
}

impl AccelDecision {
    fn hardware() -> Self {
        Self {
            mode: AccelMode::Whpx,
            reason: "Windows Hypervisor Platform is available, so the lab runs with hardware \
                     acceleration."
                .into(),
        }
    }

    fn software(detail: &str) -> Self {
        Self {
            mode: AccelMode::Tcg,
            reason: format!(
                "Running with software translation ({detail}). Everything works, but the \
                 terminal starts more slowly. No action is needed."
            ),
        }
    }
}

/// Picks the accelerator for this host. Falls back to TCG for every failure path.
pub fn detect() -> AccelDecision {
    if let Some(forced) = forced_from_env() {
        return forced;
    }
    detect_platform()
}

/// `LINUXLAB_ACCEL=tcg|whpx` overrides detection. Used by QA to exercise both paths on one
/// machine (spec 21.5 asks for both).
fn forced_from_env() -> Option<AccelDecision> {
    match std::env::var("LINUXLAB_ACCEL")
        .ok()?
        .to_lowercase()
        .as_str()
    {
        "tcg" => Some(AccelDecision {
            mode: AccelMode::Tcg,
            reason: "Software translation forced by LINUXLAB_ACCEL.".into(),
        }),
        "whpx" => Some(AccelDecision {
            mode: AccelMode::Whpx,
            reason: "Hardware acceleration forced by LINUXLAB_ACCEL.".into(),
        }),
        other => {
            tracing::warn!("ignoring unrecognised LINUXLAB_ACCEL value {other:?}");
            None
        }
    }
}

#[cfg(windows)]
fn detect_platform() -> AccelDecision {
    match windows_impl::hypervisor_present() {
        Ok(true) => AccelDecision::hardware(),
        Ok(false) => AccelDecision::software("Windows Hypervisor Platform is not enabled"),
        Err(detail) => AccelDecision::software(&detail),
    }
}

#[cfg(not(windows))]
fn detect_platform() -> AccelDecision {
    // Developer and CI hosts. The shipped product is Windows-only; KVM detection is
    // deliberately not implemented so non-Windows runs stay comparable to the worst case.
    AccelDecision::software("this is not a Windows host")
}

#[cfg(windows)]
mod windows_impl {
    use std::ffi::c_void;
    use windows_sys::core::PCSTR;
    use windows_sys::Win32::Foundation::S_OK;
    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryA};

    /// WHV_CAPABILITY_CODE_HYPERVISOR_PRESENT
    const WHV_CAPABILITY_CODE_HYPERVISOR_PRESENT: u32 = 0x0000_0000;

    type WHvGetCapabilityFn = unsafe extern "system" fn(
        capability_code: u32,
        capability_buffer: *mut c_void,
        capability_buffer_size_bytes: u32,
        written_size_bytes: *mut u32,
    ) -> i32;

    /// Asks WinHvPlatform whether a hypervisor is present.
    ///
    /// The DLL is loaded at runtime rather than linked, because it simply does not exist on
    /// a machine where the optional feature was never installed — and a missing import would
    /// stop the whole application from starting.
    pub fn hypervisor_present() -> Result<bool, String> {
        unsafe {
            let module = LoadLibraryA(c"WinHvPlatform.dll".as_ptr() as PCSTR);
            if module.is_null() {
                return Ok(false);
            }
            let symbol = GetProcAddress(module, c"WHvGetCapability".as_ptr() as PCSTR);
            let Some(symbol) = symbol else {
                return Err("WinHvPlatform.dll is present but incomplete".to_string());
            };
            let get_capability: WHvGetCapabilityFn = std::mem::transmute(symbol);

            let mut present: u8 = 0;
            let mut written: u32 = 0;
            let hr = get_capability(
                WHV_CAPABILITY_CODE_HYPERVISOR_PRESENT,
                &mut present as *mut u8 as *mut c_void,
                std::mem::size_of::<u8>() as u32,
                &mut written,
            );
            if hr != S_OK {
                // WHV_E_UNKNOWN_CAPABILITY and friends all mean "not usable here".
                return Ok(false);
            }
            Ok(written > 0 && present != 0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detection_always_yields_a_usable_mode_and_an_explanation() {
        let d = detect();
        assert!(matches!(d.mode, AccelMode::Whpx | AccelMode::Tcg));
        assert!(!d.reason.is_empty());
    }

    #[test]
    fn software_fallback_never_asks_the_learner_to_do_anything() {
        let d = AccelDecision::software("Windows Hypervisor Platform is not enabled");
        assert_eq!(d.mode, AccelMode::Tcg);
        assert!(d.reason.contains("No action is needed"));
        assert!(!d.reason.to_lowercase().contains("enable "));
    }
}
