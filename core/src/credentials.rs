use serde::Serialize;

use crate::config::{
    ALIBABA_PROVIDER, DEEPL_PROVIDER, GEMINI_PROVIDER, MICROSOFT_PROVIDER,
    OPENAI_COMPATIBLE_PROVIDER, OPENAI_PROVIDER,
};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CredentialStatus {
    pub configured: bool,
    pub stored_configured: bool,
    pub environment_override: bool,
    pub source: Option<&'static str>,
}

const EXTERNAL_API_TARGET: &str = "VRCS/ExternalAPI/token";
const EXTERNAL_API_ENV: &str = "VRCS_EXTERNAL_API_TOKEN";

pub fn external_api_token_status() -> Result<CredentialStatus, String> {
    let environment_override =
        std::env::var(EXTERNAL_API_ENV).is_ok_and(|value| !value.trim().is_empty());
    let stored_configured = read_stored(EXTERNAL_API_TARGET)?.is_some();
    Ok(CredentialStatus {
        configured: environment_override || stored_configured,
        stored_configured,
        environment_override,
        source: if environment_override {
            Some("environment")
        } else {
            stored_configured.then_some("credential_manager")
        },
    })
}

pub fn read_external_api_token() -> Result<Option<String>, String> {
    if let Ok(value) = std::env::var(EXTERNAL_API_ENV) {
        if !value.trim().is_empty() {
            return Ok(Some(value));
        }
    }
    read_stored(EXTERNAL_API_TARGET)
}

pub fn write_external_api_token(value: &str) -> Result<(), String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 4096 {
        return Err("External API token length is invalid".into());
    }
    write_stored(EXTERNAL_API_TARGET, value)
}

pub fn delete_external_api_token() -> Result<(), String> {
    delete_stored(EXTERNAL_API_TARGET)
}

fn env_names(provider: &str) -> Result<&'static [&'static str], String> {
    match provider {
        ALIBABA_PROVIDER => Ok(&["VRCS_QWEN_API_KEY", "DASHSCOPE_API_KEY"]),
        OPENAI_PROVIDER => Ok(&["VRCS_OPENAI_API_KEY", "OPENAI_API_KEY"]),
        OPENAI_COMPATIBLE_PROVIDER => Ok(&["VRCS_OPENAI_COMPATIBLE_API_KEY"]),
        GEMINI_PROVIDER => Ok(&["VRCS_GEMINI_API_KEY", "GEMINI_API_KEY"]),
        DEEPL_PROVIDER => Ok(&["VRCS_DEEPL_API_KEY", "DEEPL_API_KEY"]),
        MICROSOFT_PROVIDER => Ok(&["VRCS_MICROSOFT_TRANSLATOR_KEY"]),
        _ => Err(format!("Unsupported API provider: {provider}")),
    }
}

fn validate_provider(provider: &str) -> Result<(), String> {
    env_names(provider).map(|_| ())
}

pub fn credential_status(profile_id: &str, provider: &str) -> Result<CredentialStatus, String> {
    validate_provider(provider)?;
    let environment_override = env_names(provider)?
        .iter()
        .any(|name| std::env::var(name).is_ok_and(|value| !value.trim().is_empty()));
    let stored_configured = read_profile_stored(profile_id, provider)?.is_some();
    Ok(CredentialStatus {
        configured: environment_override || stored_configured,
        stored_configured,
        environment_override,
        source: if environment_override {
            Some("environment")
        } else {
            stored_configured.then_some("credential_manager")
        },
    })
}

pub fn read_credential(profile_id: &str, provider: &str) -> Result<Option<String>, String> {
    for name in env_names(provider)? {
        if let Ok(value) = std::env::var(name) {
            if !value.trim().is_empty() {
                return Ok(Some(value));
            }
        }
    }
    read_profile_stored(profile_id, provider)
}

pub fn write_credential(profile_id: &str, provider: &str, value: &str) -> Result<(), String> {
    validate_provider(provider)?;
    let value = value.trim();
    if value.is_empty() || value.len() > 4096 {
        return Err("API key length is invalid".into());
    }
    write_stored(&target(profile_id), value)
}

pub fn delete_credential(profile_id: &str, provider: &str) -> Result<(), String> {
    validate_provider(provider)?;
    delete_stored(&target(profile_id))?;
    delete_stored(&old_profile_target(profile_id))?;
    if let Some(target) = legacy_target(profile_id, provider) {
        delete_stored(&target)?;
    }
    Ok(())
}

fn target(profile_id: &str) -> String {
    format!("VRCS/API/profile/{profile_id}")
}

fn old_profile_target(profile_id: &str) -> String {
    format!("VRCS/ASR/profile/{profile_id}")
}

fn legacy_target(profile_id: &str, provider: &str) -> Option<String> {
    match (profile_id, provider) {
        ("legacy-alibaba-cloud", ALIBABA_PROVIDER) => Some("VRCS/ASR/qwen".into()),
        ("legacy-openai", OPENAI_PROVIDER) => Some("VRCS/ASR/openai".into()),
        _ => None,
    }
}

fn read_profile_stored(profile_id: &str, provider: &str) -> Result<Option<String>, String> {
    for target in [target(profile_id), old_profile_target(profile_id)] {
        if let Some(value) = read_stored(&target)? {
            return Ok(Some(value));
        }
    }
    match legacy_target(profile_id, provider) {
        Some(target) => read_stored(&target),
        None => Ok(None),
    }
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn read_stored(target_name: &str) -> Result<Option<String>, String> {
    use windows::core::PCWSTR;
    use windows::Win32::Security::Credentials::{
        CredFree, CredReadW, CREDENTIALW, CRED_TYPE_GENERIC,
    };

    let target = wide(target_name);
    let mut credential: *mut CREDENTIALW = std::ptr::null_mut();
    if let Err(error) = unsafe {
        CredReadW(
            PCWSTR(target.as_ptr()),
            CRED_TYPE_GENERIC,
            None,
            &mut credential,
        )
    } {
        if error.code().0 == 0x80070490u32 as i32 {
            return Ok(None);
        }
        return Err(format!("Failed to read Windows credential: {error}"));
    }
    if credential.is_null() {
        return Ok(None);
    }
    let result = unsafe {
        let credential = &*credential;
        let bytes = std::slice::from_raw_parts(
            credential.CredentialBlob,
            credential.CredentialBlobSize as usize,
        );
        String::from_utf8(bytes.to_vec())
            .map_err(|_| "Windows credential is not valid UTF-8".to_string())
    };
    unsafe { CredFree(credential.cast()) };
    result.map(Some)
}

#[cfg(windows)]
fn write_stored(target_name: &str, value: &str) -> Result<(), String> {
    use windows::core::PWSTR;
    use windows::Win32::Security::Credentials::{
        CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
    };

    let mut target = wide(target_name);
    let mut username = wide("VRCS");
    let mut blob = value.as_bytes().to_vec();
    let credential = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(target.as_mut_ptr()),
        CredentialBlobSize: blob.len() as u32,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        UserName: PWSTR(username.as_mut_ptr()),
        ..Default::default()
    };
    unsafe { CredWriteW(&credential, 0) }
        .map_err(|error| format!("Failed to write Windows credential: {error}"))
}

#[cfg(windows)]
fn delete_stored(target_name: &str) -> Result<(), String> {
    use windows::core::PCWSTR;
    use windows::Win32::Security::Credentials::{CredDeleteW, CRED_TYPE_GENERIC};

    let target = wide(target_name);
    match unsafe { CredDeleteW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, None) } {
        Ok(()) => Ok(()),
        Err(error) if error.code().0 == 0x80070490u32 as i32 => Ok(()),
        Err(error) => Err(format!("Failed to delete Windows credential: {error}")),
    }
}

#[cfg(not(windows))]
fn read_stored(_target_name: &str) -> Result<Option<String>, String> {
    Ok(None)
}

#[cfg(not(windows))]
fn write_stored(_target_name: &str, _value: &str) -> Result<(), String> {
    Err("This platform only supports API keys through environment variables".into())
}

#[cfg(not(windows))]
fn delete_stored(_target_name: &str) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_all_supported_providers() {
        for provider in [
            ALIBABA_PROVIDER,
            OPENAI_PROVIDER,
            OPENAI_COMPATIBLE_PROVIDER,
            GEMINI_PROVIDER,
            DEEPL_PROVIDER,
            MICROSOFT_PROVIDER,
        ] {
            assert!(credential_status("profile", provider).is_ok());
        }
        assert!(credential_status("profile", "unknown").is_err());
        assert!(write_credential("profile", ALIBABA_PROVIDER, " ").is_err());
    }

    #[test]
    fn legacy_profiles_keep_the_previous_targets() {
        assert_eq!(
            legacy_target("legacy-alibaba-cloud", ALIBABA_PROVIDER).as_deref(),
            Some("VRCS/ASR/qwen")
        );
        assert!(legacy_target("new-profile", ALIBABA_PROVIDER).is_none());
    }

    #[test]
    fn external_api_uses_an_independent_credential_target() {
        assert_eq!(EXTERNAL_API_TARGET, "VRCS/ExternalAPI/token");
        assert_ne!(EXTERNAL_API_TARGET, target("profile"));
        assert!(write_external_api_token(" ").is_err());
    }
}
