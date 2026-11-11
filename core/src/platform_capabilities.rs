//! Platform support is separate from resource readiness. Model readiness reflects
//! the existing runtime snapshots, not merely an installed file or compiled feature.

use serde::Serialize;

pub const AUDIO_UNAVAILABLE: &str = "platform.audio_unavailable";
pub const CREDENTIALS_UNAVAILABLE: &str = "platform.credentials_unavailable";
pub const QWEN_UNAVAILABLE: &str = "platform.qwen_unavailable";
pub const LOCAL_TRANSLATION_UNAVAILABLE: &str = "platform.local_translation_unavailable";
pub const NETWORK_CAPTURE_UNSUPPORTED: &str = "platform.network_capture_unsupported";
pub const NETWORK_HELPER_MISSING: &str = "platform.network_helper_missing";
pub const DIRECTML_UNSUPPORTED: &str = "platform.directml_unsupported";
pub const DEVICE_KEY_UNSUPPORTED: &str = "platform.audio_device_reselection_required";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Capability {
    pub supported: bool,
    pub available: bool,
    pub reason_code: Option<&'static str>,
}

impl Capability {
    pub const fn runtime(supported: bool, available: bool, reason: &'static str) -> Self {
        Self {
            supported,
            available: supported && available,
            reason_code: if supported && available {
                None
            } else {
                Some(reason)
            },
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PlatformCapabilities {
    pub os: &'static str,
    pub arch: &'static str,
    pub audio: AudioCapabilities,
    pub credentials: Capability,
    pub inference: InferenceCapabilities,
    pub chatbox: ChatboxCapabilities,
}

#[derive(Debug, Serialize)]
pub struct AudioCapabilities {
    pub microphone: Capability,
    pub system_output: Capability,
    pub application_output: Capability,
    pub player_audio: Capability,
}

#[derive(Debug, Serialize)]
pub struct InferenceCapabilities {
    pub vad_cpu: Capability,
    pub qwen_cpu: Capability,
    pub qwen_vulkan: Capability,
    pub local_translation: Capability,
    pub ocr_cpu: Capability,
    pub ocr_directml: Capability,
}

#[derive(Debug, Serialize)]
pub struct ChatboxCapabilities {
    pub osc_send: Capability,
    pub network_receive: Capability,
}

pub(crate) const fn managed_inference_supported() -> bool {
    cfg!(all(windows, target_arch = "x86_64")) || cfg!(all(target_os = "linux", target_env = "gnu", any(target_arch = "x86_64", target_arch = "aarch64")))
}

pub(crate) const fn qwen_vulkan_supported() -> bool {
    cfg!(feature = "vulkan") && managed_inference_supported()
}

pub(crate) const fn local_translation_supported() -> bool {
    managed_inference_supported() && cfg!(target_arch = "x86_64")
}

pub(crate) fn require_local_translation() -> Result<(), &'static str> {
    if local_translation_supported() {
        Ok(())
    } else {
        Err(LOCAL_TRANSLATION_UNAVAILABLE)
    }
}

pub(crate) const fn network_capture_supported() -> bool {
    cfg!(windows)
        || cfg!(all(
            target_os = "linux",
            target_arch = "x86_64",
            target_env = "gnu"
        ))
}

pub(crate) fn network_capture_available() -> bool {
    if cfg!(windows) {
        true
    } else if cfg!(target_os = "linux") {
        crate::player_audio::network_available()
    } else {
        false
    }
}

pub(crate) fn require_network_capture_supported() -> Result<(), &'static str> {
    if network_capture_supported() {
        Ok(())
    } else {
        Err(NETWORK_CAPTURE_UNSUPPORTED)
    }
}

pub(crate) fn require_network_capture() -> Result<(), &'static str> {
    require_network_capture_supported()?;
    if network_capture_available() {
        Ok(())
    } else {
        Err(NETWORK_HELPER_MISSING)
    }
}

pub(crate) fn require_audio() -> Result<(), &'static str> {
    if cfg!(any(windows, target_os = "linux")) {
        Ok(())
    } else {
        Err(AUDIO_UNAVAILABLE)
    }
}
