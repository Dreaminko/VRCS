#[cfg(windows)]
use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(windows)]
use std::sync::{Mutex, OnceLock};

use serde::Serialize;

use crate::providers::{self, ALIBABA_PROVIDER, OPENAI_PROVIDER};

#[cfg(target_os = "linux")]
mod linux;

static REVISION: AtomicU64 = AtomicU64::new(0);

pub(crate) fn revision() -> u64 {
    REVISION.load(Ordering::Acquire)
}

fn bump_revision() {
    REVISION.fetch_add(1, Ordering::Release);
}

/// Run the complete synchronous keyring operation away from Tokio workers.
pub(crate) async fn blocking<T: Send + 'static>(
    operation: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|error| {
            format!("credentials.task_failed: Credential task exited unexpectedly: {error}")
        })?
}

/// Keep mutation locks and recovery alive if an HTTP request is dropped while
/// its non-cancellable blocking keyring operation is still running.
pub(crate) async fn finish_mutation<T: Send + 'static>(
    operation: impl Future<Output = T> + Send + 'static,
) -> Result<T, String> {
    tokio::spawn(operation).await.map_err(|error| {
        format!("credentials.task_failed: Credential mutation exited unexpectedly: {error}")
    })
}

pub(crate) async fn credential_status_async(
    profile_id: &str,
    provider: &str,
) -> Result<CredentialStatus, String> {
    let (profile_id, provider) = (profile_id.to_owned(), provider.to_owned());
    blocking(move || credential_status(&profile_id, &provider)).await
}

pub(crate) async fn read_credential_async(
    profile_id: &str,
    provider: &str,
) -> Result<Option<String>, String> {
    let (profile_id, provider) = (profile_id.to_owned(), provider.to_owned());
    blocking(move || read_credential(&profile_id, &provider)).await
}

pub(crate) async fn read_credential_for_use_async(
    profile_id: &str,
    provider: &str,
) -> Result<Option<String>, String> {
    let (profile_id, provider) = (profile_id.to_owned(), provider.to_owned());
    blocking(move || read_credential_for_use(&profile_id, &provider)).await
}

pub(crate) async fn read_profile_stored_for_use_async(
    profile_id: &str,
    provider: &str,
) -> Result<Option<String>, String> {
    let (profile_id, provider) = (profile_id.to_owned(), provider.to_owned());
    blocking(move || read_profile_stored_for_use(&profile_id, &provider)).await
}

pub(crate) async fn write_credential_async(
    profile_id: &str,
    provider: &str,
    value: &str,
) -> Result<(), String> {
    let (profile_id, provider, value) =
        (profile_id.to_owned(), provider.to_owned(), value.to_owned());
    blocking(move || write_credential(&profile_id, &provider, &value)).await
}

pub(crate) async fn delete_credential_async(
    profile_id: &str,
    provider: &str,
) -> Result<(), String> {
    let (profile_id, provider) = (profile_id.to_owned(), provider.to_owned());
    blocking(move || delete_credential(&profile_id, &provider)).await
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CredentialStatus {
    pub configured: bool,
    pub stored_configured: bool,
    pub environment_override: bool,
    pub source: Option<&'static str>,
    pub storage_available: bool,
    pub reason_code: Option<&'static str>,
}

const EXTERNAL_API_TARGET: &str = "VRCS/ExternalAPI/token";
const EXTERNAL_API_ENV: &str = "VRCS_EXTERNAL_API_TOKEN";
const VRCX_TARGET: &str = "VRCS/VRCXIntegration/token";
const VRCX_ENV: &str = "VRCS_VRCX_INTEGRATION_TOKEN";
pub fn external_api_token_status() -> Result<CredentialStatus, String> {
    let environment_override =
        std::env::var(EXTERNAL_API_ENV).is_ok_and(|value| !value.trim().is_empty());
    Ok(status_from_storage(
        environment_override,
        stored_status(&[EXTERNAL_API_TARGET.into()]),
    ))
}

pub fn read_external_api_token() -> Result<Option<String>, String> {
    if let Ok(value) = std::env::var(EXTERNAL_API_ENV)
        && !value.trim().is_empty()
    {
        return Ok(Some(value));
    }
    read_stored_external_api_token()
}

pub(crate) fn read_stored_external_api_token() -> Result<Option<String>, String> {
    read_stored(EXTERNAL_API_TARGET)
}

pub fn write_external_api_token(value: &str) -> Result<(), String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 4096 {
        return Err("External API token length is invalid".into());
    }
    write_stored(EXTERNAL_API_TARGET, value)?;
    bump_revision();
    Ok(())
}

pub fn delete_external_api_token() -> Result<(), String> {
    delete_stored(EXTERNAL_API_TARGET)?;
    bump_revision();
    Ok(())
}

pub fn vrcx_token_status() -> Result<CredentialStatus, String> {
    let environment_override = std::env::var(VRCX_ENV).is_ok_and(|value| !value.trim().is_empty());
    Ok(status_from_storage(
        environment_override,
        stored_status(&[VRCX_TARGET.into()]),
    ))
}

pub fn read_vrcx_token() -> Result<Option<String>, String> {
    if let Ok(value) = std::env::var(VRCX_ENV)
        && !value.trim().is_empty()
    {
        return Ok(Some(value));
    }
    read_stored_vrcx_token()
}

pub(crate) fn read_stored_vrcx_token() -> Result<Option<String>, String> {
    read_stored(VRCX_TARGET)
}

pub fn write_vrcx_token(value: &str) -> Result<(), String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 4096 {
        return Err("VRCX-0 token length is invalid".into());
    }
    write_stored(VRCX_TARGET, value)?;
    bump_revision();
    Ok(())
}

pub fn delete_vrcx_token() -> Result<(), String> {
    delete_stored(VRCX_TARGET)?;
    bump_revision();
    Ok(())
}

fn environment_variables(
    provider: &str,
) -> Result<(&'static [&'static str], &'static [&'static str]), String> {
    let definition = providers::definition(provider)
        .ok_or_else(|| format!("Unsupported API provider: {provider}"))?;
    Ok((
        definition.connection.environment_variables,
        definition.connection.legacy_environment_variables,
    ))
}

fn validate_provider(provider: &str) -> Result<(), String> {
    environment_variables(provider).map(|_| ())
}

pub fn credential_status(profile_id: &str, provider: &str) -> Result<CredentialStatus, String> {
    validate_provider(provider)?;
    let (primary, legacy) = environment_variables(provider)?;
    let environment_override = primary
        .iter()
        .chain(legacy)
        .any(|name| std::env::var(name).is_ok_and(|value| !value.trim().is_empty()));
    Ok(status_from_storage(
        environment_override,
        stored_status(&profile_targets(profile_id, provider)),
    ))
}

pub fn read_credential(profile_id: &str, provider: &str) -> Result<Option<String>, String> {
    let (primary, legacy) = environment_variables(provider)?;
    for name in primary.iter().chain(legacy) {
        if let Ok(value) = std::env::var(name)
            && !value.trim().is_empty()
        {
            return Ok(Some(value));
        }
    }
    read_stored_credential(profile_id, provider)
}

pub(crate) fn read_stored_credential(
    profile_id: &str,
    provider: &str,
) -> Result<Option<String>, String> {
    read_profile_stored(profile_id, provider)
}

/// Read for a user-initiated operation; Linux may ask to unlock the keyring.
pub fn read_credential_for_use(profile_id: &str, provider: &str) -> Result<Option<String>, String> {
    let (primary, legacy) = environment_variables(provider)?;
    for name in primary.iter().chain(legacy) {
        if let Ok(value) = std::env::var(name)
            && !value.trim().is_empty()
        {
            return Ok(Some(value));
        }
    }
    read_profile_stored_for_use(profile_id, provider)
}

pub(crate) fn read_profile_stored_for_use(
    profile_id: &str,
    provider: &str,
) -> Result<Option<String>, String> {
    validate_provider(provider)?;
    for target in profile_targets(profile_id, provider) {
        if let Some(value) = read_stored_for_use(&target)? {
            return Ok(Some(value));
        }
    }
    Ok(None)
}

pub fn read_external_api_token_for_use() -> Result<Option<String>, String> {
    if let Ok(value) = std::env::var(EXTERNAL_API_ENV)
        && !value.trim().is_empty()
    {
        return Ok(Some(value));
    }
    read_stored_for_use(EXTERNAL_API_TARGET)
}

pub(crate) fn read_stored_external_api_token_for_use() -> Result<Option<String>, String> {
    read_stored_for_use(EXTERNAL_API_TARGET)
}

pub fn read_vrcx_token_for_use() -> Result<Option<String>, String> {
    if let Ok(value) = std::env::var(VRCX_ENV)
        && !value.trim().is_empty()
    {
        return Ok(Some(value));
    }
    read_stored_for_use(VRCX_TARGET)
}

pub fn write_credential(profile_id: &str, provider: &str, value: &str) -> Result<(), String> {
    validate_provider(provider)?;
    let value = value.trim();
    if value.is_empty() || value.len() > 4096 {
        return Err("API key length is invalid".into());
    }
    write_stored(&target(profile_id), value)?;
    bump_revision();
    Ok(())
}

pub fn delete_credential(profile_id: &str, provider: &str) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        linux::delete(&profile_targets(profile_id, provider))?;
        bump_revision();
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        delete_stored(&target(profile_id))?;
        delete_stored(&old_profile_target(profile_id))?;
        if let Some(target) = legacy_target(profile_id, provider) {
            delete_stored(&target)?;
        }
        bump_revision();
        Ok(())
    }
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

fn profile_targets(profile_id: &str, provider: &str) -> Vec<String> {
    let mut targets = vec![target(profile_id), old_profile_target(profile_id)];
    if let Some(target) = legacy_target(profile_id, provider) {
        targets.push(target);
    }
    targets
}

struct StoredStatus {
    configured: bool,
    available: bool,
    reason_code: Option<&'static str>,
}

fn status_from_storage(environment_override: bool, stored: StoredStatus) -> CredentialStatus {
    CredentialStatus {
        configured: environment_override || stored.configured,
        stored_configured: stored.configured,
        environment_override,
        source: if environment_override {
            Some("environment")
        } else {
            stored.configured.then_some(if cfg!(target_os = "linux") {
                "secret_service"
            } else {
                "credential_manager"
            })
        },
        storage_available: stored.available,
        reason_code: stored.reason_code,
    }
}

/// Stable machine-readable reasons; never parse the platform's localized detail.
pub fn error_reason_code(error: &str) -> Option<&'static str> {
    [
        "credentials.not_set",
        "credentials.locked",
        "credentials.no_service",
        "credentials.cancelled",
        "credentials.read_failed",
        "credentials.write_failed",
        "credentials.rollback_failed",
        "credentials.task_failed",
        "credentials.unsupported",
    ]
    .into_iter()
    .find(|code| {
        error == *code
            || error
                .strip_prefix(*code)
                .is_some_and(|tail| tail.starts_with(':'))
    })
}

/// Probe only: never creates a collection, unlocks it or displays a prompt.
pub fn storage_capability() -> (bool, Option<&'static str>) {
    #[cfg(target_os = "linux")]
    return linux::capability();
    #[cfg(windows)]
    return (true, None);
    #[cfg(not(any(windows, target_os = "linux")))]
    return (false, Some("credentials.unsupported"));
}

fn stored_status(targets: &[String]) -> StoredStatus {
    #[cfg(target_os = "linux")]
    return linux::status(targets);
    #[cfg(not(target_os = "linux"))]
    {
        let (available, reason_code) = storage_capability();
        for target in targets {
            match read_stored(target) {
                Ok(Some(_)) => {
                    return StoredStatus {
                        configured: true,
                        available,
                        reason_code,
                    };
                }
                Ok(None) => (),
                Err(_) => {
                    return StoredStatus {
                        configured: false,
                        available: false,
                        reason_code: Some("credentials.read_failed"),
                    };
                }
            }
        }
        StoredStatus {
            configured: false,
            available,
            reason_code: reason_code.or(Some("credentials.not_set")),
        }
    }
}

fn read_stored_for_use(target: &str) -> Result<Option<String>, String> {
    #[cfg(target_os = "linux")]
    return linux::read(target, true);
    #[cfg(not(target_os = "linux"))]
    return read_stored(target);
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
fn stored_cache() -> &'static Mutex<HashMap<String, Option<String>>> {
    static STORED_CACHE: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
    STORED_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(windows)]
fn read_stored(target_name: &str) -> Result<Option<String>, String> {
    use windows::Win32::Security::Credentials::{
        CRED_TYPE_GENERIC, CREDENTIALW, CredFree, CredReadW,
    };
    use windows::core::PCWSTR;

    if let Some(cached) = stored_cache()
        .lock()
        .expect("credential cache lock")
        .get(target_name)
    {
        return Ok(cached.clone());
    }

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
            stored_cache()
                .lock()
                .expect("credential cache lock")
                .insert(target_name.to_owned(), None);
            return Ok(None);
        }
        return Err(format!("Failed to read Windows credential: {error}"));
    }
    if credential.is_null() {
        stored_cache()
            .lock()
            .expect("credential cache lock")
            .insert(target_name.to_owned(), None);
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
    let value = result?;
    stored_cache()
        .lock()
        .expect("credential cache lock")
        .insert(target_name.to_owned(), Some(value.clone()));
    Ok(Some(value))
}

#[cfg(windows)]
fn write_stored(target_name: &str, value: &str) -> Result<(), String> {
    use windows::Win32::Security::Credentials::{
        CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredWriteW,
    };
    use windows::core::PWSTR;

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
        .map_err(|error| format!("Failed to write Windows credential: {error}"))?;
    stored_cache()
        .lock()
        .expect("credential cache lock")
        .insert(target_name.to_owned(), Some(value.to_owned()));
    Ok(())
}

#[cfg(windows)]
fn delete_stored(target_name: &str) -> Result<(), String> {
    use windows::Win32::Security::Credentials::{CRED_TYPE_GENERIC, CredDeleteW};
    use windows::core::PCWSTR;

    let target = wide(target_name);
    match unsafe { CredDeleteW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, None) } {
        Ok(()) => {
            stored_cache()
                .lock()
                .expect("credential cache lock")
                .insert(target_name.to_owned(), None);
            Ok(())
        }
        Err(error) if error.code().0 == 0x80070490u32 as i32 => {
            stored_cache()
                .lock()
                .expect("credential cache lock")
                .insert(target_name.to_owned(), None);
            Ok(())
        }
        Err(error) => Err(format!("Failed to delete Windows credential: {error}")),
    }
}

#[cfg(target_os = "linux")]
fn read_stored(target_name: &str) -> Result<Option<String>, String> {
    linux::read(target_name, false)
}

#[cfg(target_os = "linux")]
fn write_stored(target_name: &str, value: &str) -> Result<(), String> {
    linux::write(target_name, value)
}

#[cfg(target_os = "linux")]
fn delete_stored(target_name: &str) -> Result<(), String> {
    linux::delete(&[target_name.to_owned()])
}

#[cfg(not(any(windows, target_os = "linux")))]
fn read_stored(_target_name: &str) -> Result<Option<String>, String> {
    Ok(None)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn write_stored(_target_name: &str, _value: &str) -> Result<(), String> {
    Err("credentials.unsupported: This platform only supports API keys through environment variables".into())
}

#[cfg(not(any(windows, target_os = "linux")))]
fn delete_stored(_target_name: &str) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn cancelled_keyring_mutation_keeps_control_without_blocking_the_runtime() {
        let control = std::sync::Arc::new(tokio::sync::Mutex::new(()));
        let mutation_control = control.clone();
        let (started, ready) = tokio::sync::oneshot::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let (done, completed) = tokio::sync::oneshot::channel();
        let request = tokio::spawn(finish_mutation(async move {
            let _control = mutation_control.lock().await;
            let result = blocking(move || {
                let _ = started.send(());
                wait.recv_timeout(std::time::Duration::from_secs(5))
                    .map_err(|error| error.to_string())
            })
            .await;
            let _ = done.send(result);
        }));
        ready.await.unwrap();
        // The sole runtime worker must reach this while keyring I/O is waiting.
        request.abort();
        let _ = request.await;
        assert!(control.try_lock().is_err());
        release.send(()).unwrap();
        completed.await.unwrap().unwrap();
        let _released = control.lock().await;
    }

    #[test]
    fn accepts_all_supported_providers() {
        for provider in providers::catalog() {
            assert!(credential_status("profile", provider.id).is_ok());
        }
        assert!(credential_status("profile", "unknown").is_err());
        assert!(write_credential("profile", ALIBABA_PROVIDER, " ").is_err());
    }

    #[test]
    fn brand_environment_variables_keep_the_compatible_fallback() {
        let (primary, legacy) = environment_variables(providers::GROQ_PROVIDER).unwrap();
        assert_eq!(primary, ["VRCS_GROQ_API_KEY", "GROQ_API_KEY"]);
        assert_eq!(legacy, ["VRCS_OPENAI_COMPATIBLE_API_KEY"]);
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
    fn integration_tokens_use_independent_credential_targets() {
        assert_eq!(EXTERNAL_API_TARGET, "VRCS/ExternalAPI/token");
        assert_eq!(VRCX_TARGET, "VRCS/VRCXIntegration/token");
        assert_ne!(EXTERNAL_API_TARGET, VRCX_TARGET);
        assert_ne!(VRCX_TARGET, target("profile"));
        assert!(write_external_api_token(" ").is_err());
        assert!(write_vrcx_token(" ").is_err());
    }
}
