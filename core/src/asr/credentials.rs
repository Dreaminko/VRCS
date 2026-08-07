use serde::Serialize;

const PROVIDERS: [&str; 2] = ["qwen", "openai"];

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CredentialStatus {
    pub configured: bool,
    pub source: Option<&'static str>,
}

fn env_names(provider: &str) -> Result<&'static [&'static str], String> {
    match provider {
        "qwen" => Ok(&["VRCS_QWEN_API_KEY", "DASHSCOPE_API_KEY"]),
        "openai" => Ok(&["VRCS_OPENAI_API_KEY", "OPENAI_API_KEY"]),
        _ => Err(format!("不支持的云端识别服务：{provider}")),
    }
}

fn validate_provider(provider: &str) -> Result<(), String> {
    if PROVIDERS.contains(&provider) {
        Ok(())
    } else {
        Err(format!("不支持的云端识别服务：{provider}"))
    }
}

pub fn credential_status(provider: &str) -> Result<CredentialStatus, String> {
    if env_names(provider)?
        .iter()
        .any(|name| std::env::var(name).is_ok_and(|value| !value.trim().is_empty()))
    {
        return Ok(CredentialStatus {
            configured: true,
            source: Some("environment"),
        });
    }
    let configured = read_stored(provider)?.is_some();
    Ok(CredentialStatus {
        configured,
        source: configured.then_some("credential_manager"),
    })
}

pub fn read_credential(provider: &str) -> Result<Option<String>, String> {
    for name in env_names(provider)? {
        if let Ok(value) = std::env::var(name) {
            if !value.trim().is_empty() {
                return Ok(Some(value));
            }
        }
    }
    read_stored(provider)
}

pub fn write_credential(provider: &str, value: &str) -> Result<(), String> {
    validate_provider(provider)?;
    let value = value.trim();
    if value.is_empty() || value.len() > 4096 {
        return Err("API Key 长度无效".into());
    }
    write_stored(provider, value)
}

pub fn delete_credential(provider: &str) -> Result<(), String> {
    validate_provider(provider)?;
    delete_stored(provider)
}

fn target(provider: &str) -> String {
    format!("VRCS/ASR/{provider}")
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn read_stored(provider: &str) -> Result<Option<String>, String> {
    use windows::core::PCWSTR;
    use windows::Win32::Security::Credentials::{
        CredFree, CredReadW, CREDENTIALW, CRED_TYPE_GENERIC,
    };

    let target = wide(&target(provider));
    let mut credential: *mut CREDENTIALW = std::ptr::null_mut();
    if let Err(error) = unsafe {
        CredReadW(
            PCWSTR(target.as_ptr()),
            CRED_TYPE_GENERIC,
            None,
            &mut credential,
        )
    } {
        // ERROR_NOT_FOUND
        if error.code().0 == 0x80070490u32 as i32 {
            return Ok(None);
        }
        return Err(format!("无法读取 Windows 凭据：{error}"));
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
        String::from_utf8(bytes.to_vec()).map_err(|_| "Windows 凭据不是有效 UTF-8".to_string())
    };
    unsafe { CredFree(credential.cast()) };
    result.map(Some)
}

#[cfg(windows)]
fn write_stored(provider: &str, value: &str) -> Result<(), String> {
    use windows::core::PWSTR;
    use windows::Win32::Security::Credentials::{
        CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
    };

    let mut target = wide(&target(provider));
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
    unsafe { CredWriteW(&credential, 0) }.map_err(|error| format!("无法写入 Windows 凭据：{error}"))
}

#[cfg(windows)]
fn delete_stored(provider: &str) -> Result<(), String> {
    use windows::core::PCWSTR;
    use windows::Win32::Security::Credentials::{CredDeleteW, CRED_TYPE_GENERIC};

    let target = wide(&target(provider));
    match unsafe { CredDeleteW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, None) } {
        Ok(()) => Ok(()),
        Err(error) if error.code().0 == 0x80070490u32 as i32 => Ok(()),
        Err(error) => Err(format!("无法删除 Windows 凭据：{error}")),
    }
}

#[cfg(not(windows))]
fn read_stored(_provider: &str) -> Result<Option<String>, String> {
    Ok(None)
}

#[cfg(not(windows))]
fn write_stored(_provider: &str, _value: &str) -> Result<(), String> {
    Err("当前平台只支持通过环境变量配置 API Key".into())
}

#[cfg(not(windows))]
fn delete_stored(_provider: &str) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_provider_and_empty_key() {
        assert!(credential_status("unknown").is_err());
        assert!(write_credential("qwen", " ").is_err());
    }
}
