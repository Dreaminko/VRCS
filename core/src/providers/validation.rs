use crate::config::{ApiAuthMode, ApiProfile};

use super::{
    effective_purpose, ALIBABA_PROVIDER, API_PURPOSE_ASR, API_PURPOSE_LLM, API_PURPOSE_SHARED,
    DEEPL_PROVIDER, GEMINI_PROVIDER, MICROSOFT_PROVIDER, OPENAI_COMPATIBLE_PRESETS,
    OPENAI_COMPATIBLE_PROVIDER, OPENAI_PROVIDER,
};

pub(crate) fn validate_profile(profile: &ApiProfile) -> Result<(), String> {
    if !matches!(
        effective_purpose(profile),
        API_PURPOSE_ASR | API_PURPOSE_LLM | API_PURPOSE_SHARED
    ) {
        return Err(format!(
            "Unsupported API profile purpose: {}",
            effective_purpose(profile)
        ));
    }
    match profile.provider.as_str() {
        ALIBABA_PROVIDER => validate_alibaba_profile(profile)?,
        MICROSOFT_PROVIDER => validate_microsoft_profile(profile)?,
        OPENAI_PROVIDER
            if profile.region.is_none()
                && profile.workspace_id.is_none()
                && profile.base_url.is_none() => {}
        OPENAI_COMPATIBLE_PROVIDER
            if profile.region.is_none() && profile.workspace_id.is_none() =>
        {
            let base_url = profile.base_url.as_deref().unwrap_or("");
            validate_openai_base_url(base_url, profile.requires_api_key())?;
            if effective_purpose(profile) != API_PURPOSE_LLM {
                return Err("OpenAI-compatible APIs can only be used by LLM profiles".into());
            }
            validate_compatible_profile(profile)?;
        }
        GEMINI_PROVIDER
            if profile.region.is_none()
                && profile.workspace_id.is_none()
                && profile.base_url.is_none()
                && effective_purpose(profile) == API_PURPOSE_LLM => {}
        DEEPL_PROVIDER
            if profile.region.is_none()
                && profile.workspace_id.is_none()
                && profile.base_url.is_none()
                && effective_purpose(profile) == API_PURPOSE_LLM => {}
        OPENAI_PROVIDER | OPENAI_COMPATIBLE_PROVIDER | GEMINI_PROVIDER | DEEPL_PROVIDER => {
            return Err(format!(
                "API profile {} contains unsupported connection fields",
                profile.provider
            ));
        }
        other => return Err(format!("Unsupported API provider: {other}")),
    }
    if profile.timeout_ms < 1_000 || profile.timeout_ms > 120_000 {
        return Err("API profile timeout_ms must be between 1000 and 120000".into());
    }
    if profile.provider != OPENAI_COMPATIBLE_PROVIDER
        && (profile.preset_id.is_some()
            || profile.auth_mode != ApiAuthMode::Bearer
            || profile.is_local
            || !profile.headers.is_empty())
    {
        return Err(format!(
            "API profile {} contains OpenAI-compatible-only settings",
            profile.provider
        ));
    }
    Ok(())
}

fn validate_alibaba_profile(profile: &ApiProfile) -> Result<(), String> {
    if profile.base_url.is_some() {
        return Err("Alibaba Cloud profiles cannot contain an OpenAI-compatible Base URL".into());
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
    Ok(())
}

fn validate_microsoft_profile(profile: &ApiProfile) -> Result<(), String> {
    let region = profile.region.as_deref().unwrap_or("").trim();
    if region.is_empty() || region.len() > 64 {
        return Err("Microsoft Translator region must contain 1 to 64 characters".into());
    }
    if profile.workspace_id.is_some() {
        return Err("Microsoft Translator profiles cannot contain a Workspace ID".into());
    }
    if profile.base_url.is_some() {
        return Err(
            "Microsoft Translator profiles cannot contain an OpenAI-compatible Base URL".into(),
        );
    }
    Ok(())
}

fn validate_openai_base_url(base_url: &str, sends_bearer_token: bool) -> Result<(), String> {
    let base_url = base_url.trim();
    if base_url.is_empty() || base_url.len() > 2048 {
        return Err("The OpenAI-compatible Base URL must contain 1 to 2048 characters".into());
    }
    let url = reqwest::Url::parse(base_url)
        .map_err(|_| "The OpenAI-compatible Base URL is invalid".to_string())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("The OpenAI-compatible Base URL must use HTTP or HTTPS".into());
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(
            "The OpenAI-compatible Base URL cannot contain credentials, a query, or a fragment"
                .into(),
        );
    }
    if sends_bearer_token && url.scheme() == "http" && !is_loopback_url(&url) {
        return Err(
            "OpenAI-compatible profiles cannot send Bearer credentials over remote HTTP".into(),
        );
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

fn validate_compatible_profile(profile: &ApiProfile) -> Result<(), String> {
    if let Some(preset_id) = profile.preset_id.as_deref() {
        let Some(preset) = OPENAI_COMPATIBLE_PRESETS
            .iter()
            .find(|candidate| candidate.id == preset_id)
        else {
            return Err("Unsupported OpenAI-compatible preset".into());
        };
        if preset_id != "custom"
            && (profile.auth_mode != preset.auth_mode || profile.is_local != preset.is_local)
        {
            return Err("OpenAI-compatible preset metadata is inconsistent".into());
        }
    }
    if profile.headers.len() > 16 {
        return Err("OpenAI-compatible profiles can contain at most 16 custom headers".into());
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
    let mut names = std::collections::HashSet::new();
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
        if reqwest::header::HeaderValue::from_str(&header.value).is_err() {
            return Err(format!("Custom HTTP header value is invalid: {name}"));
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

    #[test]
    fn compatible_preset_catalog_is_the_validation_source() {
        for preset in OPENAI_COMPATIBLE_PRESETS {
            let profile = ApiProfile {
                id: preset.id.into(),
                name: preset.display_name.into(),
                provider: OPENAI_COMPATIBLE_PROVIDER.into(),
                base_url: Some(if preset.base_url.is_empty() {
                    "https://example.com/v1".into()
                } else {
                    preset.base_url.into()
                }),
                purpose: Some(API_PURPOSE_LLM.into()),
                preset_id: Some(preset.id.into()),
                auth_mode: preset.auth_mode,
                is_local: preset.is_local,
                ..ApiProfile::default()
            };

            assert!(validate_profile(&profile).is_ok(), "{}", preset.id);
        }

        let invalid = ApiProfile {
            provider: OPENAI_COMPATIBLE_PROVIDER.into(),
            base_url: Some("https://example.com/v1".into()),
            purpose: Some(API_PURPOSE_LLM.into()),
            preset_id: Some("missing-preset".into()),
            ..ApiProfile::default()
        };
        assert_eq!(
            validate_profile(&invalid).unwrap_err(),
            "Unsupported OpenAI-compatible preset"
        );

        let mismatched = ApiProfile {
            provider: OPENAI_COMPATIBLE_PROVIDER.into(),
            base_url: Some("http://127.0.0.1:11434/v1".into()),
            purpose: Some(API_PURPOSE_LLM.into()),
            preset_id: Some("ollama".into()),
            auth_mode: ApiAuthMode::Bearer,
            is_local: false,
            ..ApiProfile::default()
        };
        assert_eq!(
            validate_profile(&mismatched).unwrap_err(),
            "OpenAI-compatible preset metadata is inconsistent"
        );
    }
}
