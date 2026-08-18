use std::collections::HashSet;

use crate::config::{ApiAuthMode, ApiProfile};

use super::{
    definition, provider_capability_ids, BaseUrlPolicy, ProviderCategory, ALIBABA_PROVIDER,
    MICROSOFT_PROVIDER,
};

pub(crate) fn validate_profile(profile: &ApiProfile) -> Result<(), String> {
    let provider = definition(&profile.provider)
        .ok_or_else(|| format!("Unsupported API provider: {}", profile.provider))?;

    validate_capabilities(profile)?;
    match profile.provider.as_str() {
        ALIBABA_PROVIDER => validate_alibaba_profile(profile)?,
        MICROSOFT_PROVIDER => validate_microsoft_profile(profile)?,
        _ => validate_connection_fields(profile)?,
    }

    if profile.timeout_ms < 1_000 || profile.timeout_ms > 120_000 {
        return Err("API profile timeout_ms must be between 1000 and 120000".into());
    }
    if !provider.connection.allow_custom_headers && !profile.headers.is_empty() {
        return Err(format!(
            "API profile {} does not support custom HTTP headers",
            profile.provider
        ));
    }
    if provider.connection.allow_custom_headers {
        validate_custom_headers(profile)?;
    }
    Ok(())
}

fn validate_capabilities(profile: &ApiProfile) -> Result<(), String> {
    if profile.enabled_capabilities.is_empty() {
        return Err("An API profile must enable at least one capability".into());
    }
    let supported = provider_capability_ids(&profile.provider)
        .ok_or_else(|| format!("Unsupported API provider: {}", profile.provider))?;
    let mut enabled = HashSet::new();
    for capability in &profile.enabled_capabilities {
        if capability.trim().is_empty() || !supported.contains(&capability.as_str()) {
            return Err(format!(
                "Unsupported capability for provider {}: {capability}",
                profile.provider
            ));
        }
        if !enabled.insert(capability.as_str()) {
            return Err(format!(
                "API profile capability is duplicated: {capability}"
            ));
        }
    }
    Ok(())
}

fn validate_connection_fields(profile: &ApiProfile) -> Result<(), String> {
    let provider = definition(&profile.provider).expect("provider validated before connection");
    if profile.region.is_some() || profile.workspace_id.is_some() {
        return Err(format!(
            "API profile {} contains unsupported connection fields",
            profile.provider
        ));
    }
    match provider.connection.base_url {
        BaseUrlPolicy::Fixed(_) => {
            if profile.base_url.is_some() {
                return Err(format!(
                    "API profile {} cannot override its Base URL",
                    profile.provider
                ));
            }
        }
        BaseUrlPolicy::Regional => {
            return Err(format!(
                "API profile {} requires provider-specific regional validation",
                profile.provider
            ));
        }
        BaseUrlPolicy::Editable(default) => {
            let base_url = profile.base_url.as_deref().unwrap_or(default);
            validate_editable_base_url(base_url, profile.requires_api_key())?;
        }
    }

    let expected_local = provider.category == ProviderCategory::LocalService;
    let auth_is_valid = if provider.category == ProviderCategory::CustomProtocol {
        matches!(profile.auth_mode, ApiAuthMode::Bearer | ApiAuthMode::None)
    } else {
        profile.auth_mode == provider.connection.auth_mode
    };
    let local_is_valid =
        provider.category == ProviderCategory::CustomProtocol || profile.is_local == expected_local;
    if !auth_is_valid || !local_is_valid {
        return Err(format!(
            "API profile {} connection metadata is inconsistent",
            profile.provider
        ));
    }
    Ok(())
}

fn validate_alibaba_profile(profile: &ApiProfile) -> Result<(), String> {
    if profile.base_url.is_some() {
        return Err("Alibaba Cloud profiles cannot contain a Base URL".into());
    }
    let region = profile.region.as_deref().unwrap_or("");
    if !["singapore", "china_beijing"].contains(&region) {
        return Err(format!("Unsupported Alibaba Cloud region: {region}"));
    }
    let workspace = profile.workspace_id.as_deref().unwrap_or("").trim();
    let valid_workspace = workspace
        .bytes()
        .all(|value| value.is_ascii_alphanumeric() || value == b'-');
    if !workspace.is_empty() && (workspace.len() > 128 || !valid_workspace) {
        return Err("The Alibaba Cloud Workspace ID is invalid".into());
    }
    if profile.auth_mode != ApiAuthMode::Bearer || profile.is_local {
        return Err("Alibaba Cloud profile connection metadata is inconsistent".into());
    }
    Ok(())
}

fn validate_microsoft_profile(profile: &ApiProfile) -> Result<(), String> {
    let region = profile.region.as_deref().unwrap_or("").trim();
    if region.is_empty() || region.len() > 64 {
        return Err("Microsoft Translator region must contain 1 to 64 characters".into());
    }
    if profile.workspace_id.is_some() || profile.base_url.is_some() {
        return Err("Microsoft Translator profiles contain unsupported connection fields".into());
    }
    if profile.auth_mode != ApiAuthMode::Bearer || profile.is_local {
        return Err("Microsoft Translator profile connection metadata is inconsistent".into());
    }
    Ok(())
}

fn validate_editable_base_url(base_url: &str, sends_bearer_token: bool) -> Result<(), String> {
    let base_url = base_url.trim();
    if base_url.is_empty() || base_url.len() > 2048 {
        return Err("The API profile Base URL must contain 1 to 2048 characters".into());
    }
    let url = reqwest::Url::parse(base_url)
        .map_err(|_| "The API profile Base URL is invalid".to_string())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("The API profile Base URL must use HTTP or HTTPS".into());
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(
            "The API profile Base URL cannot contain credentials, a query, or a fragment".into(),
        );
    }
    if sends_bearer_token && url.scheme() == "http" && !is_loopback_url(&url) {
        return Err("API profiles cannot send Bearer credentials over remote HTTP".into());
    }
    Ok(())
}

fn is_loopback_url(url: &reqwest::Url) -> bool {
    url.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    })
}

fn validate_custom_headers(profile: &ApiProfile) -> Result<(), String> {
    if profile.headers.len() > 16 {
        return Err("API profiles can contain at most 16 custom headers".into());
    }
    const BLOCKED_HEADERS: [&str; 11] = [
        "authorization",
        "proxy-authorization",
        "cookie",
        "set-cookie",
        "x-api-key",
        "x-goog-api-key",
        "content-type",
        "accept",
        "user-agent",
        "host",
        "content-length",
    ];
    let mut names = HashSet::new();
    for header in &profile.headers {
        let name = header.name.trim();
        if name.is_empty()
            || name.len() > 128
            || !name.bytes().all(is_http_token_byte)
            || BLOCKED_HEADERS.contains(&name.to_ascii_lowercase().as_str())
        {
            return Err(format!("Custom HTTP header name is not allowed: {name}"));
        }
        if !names.insert(name.to_ascii_lowercase()) {
            return Err(format!("Custom HTTP header is duplicated: {name}"));
        }
        if header.value.len() > 2048
            || header.value.contains('\r')
            || header.value.contains('\n')
            || reqwest::header::HeaderValue::from_str(&header.value).is_err()
        {
            return Err(format!("Custom HTTP header value is invalid: {name}"));
        }
    }
    Ok(())
}

fn is_http_token_byte(value: u8) -> bool {
    value.is_ascii_alphanumeric()
        || matches!(
            value,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{
        CAPABILITY_TEXT_GENERATION, DEEPSEEK_PROVIDER, OLLAMA_PROVIDER, OPENAI_COMPATIBLE_PROVIDER,
    };

    fn profile(provider: &str) -> ApiProfile {
        ApiProfile {
            id: "profile".into(),
            name: "Profile".into(),
            provider: provider.into(),
            enabled_capabilities: vec![CAPABILITY_TEXT_GENERATION.into()],
            ..ApiProfile::default()
        }
    }

    #[test]
    fn legacy_preset_does_not_control_brand_validation() {
        let mut profile = profile(DEEPSEEK_PROVIDER);
        profile.preset_id = Some("custom-legacy-value".into());
        assert!(validate_profile(&profile).is_ok());
    }

    #[test]
    fn editable_profiles_apply_transport_safety_rules() {
        let mut custom = profile(OPENAI_COMPATIBLE_PROVIDER);
        custom.base_url = Some("http://192.0.2.1/v1".into());
        assert!(validate_profile(&custom).is_err());
        custom.auth_mode = ApiAuthMode::None;
        assert!(validate_profile(&custom).is_ok());

        let mut ollama = profile(OLLAMA_PROVIDER);
        ollama.auth_mode = ApiAuthMode::None;
        ollama.is_local = true;
        assert!(validate_profile(&ollama).is_ok());
    }
}
