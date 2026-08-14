use std::time::Duration;

use crate::config::ApiProfile;

use super::http::list_models as parse_models;
use super::openai_compatible;
use super::{LlmError, LlmProgress, LlmRequest};

pub(super) async fn generate(
    http: &reqwest::Client,
    profile: &ApiProfile,
    api_key: &str,
    request: LlmRequest<'_>,
    on_progress: Option<&LlmProgress>,
) -> Result<String, LlmError> {
    openai_compatible::generate_standard(
        http,
        format!("{}/chat/completions", base_url(profile)?),
        api_key,
        request,
        "Alibaba Cloud",
        on_progress,
    )
    .await
}

pub(super) async fn list_models(
    http: &reqwest::Client,
    profile: &ApiProfile,
    api_key: &str,
) -> Result<Vec<String>, LlmError> {
    parse_models(
        http.get(format!("{}/models", base_url(profile)?))
            .timeout(Duration::from_millis(profile.timeout_ms))
            .bearer_auth(api_key),
    )
    .await
}

fn base_url(profile: &ApiProfile) -> Result<&'static str, LlmError> {
    match profile.region.as_deref() {
        Some("china_beijing") => Ok("https://dashscope.aliyuncs.com/compatible-mode/v1"),
        Some("singapore") => Ok("https://dashscope-intl.aliyuncs.com/compatible-mode/v1"),
        _ => Err(LlmError {
            code: "llm.invalid_profile",
            detail: "Alibaba Cloud region is invalid".into(),
            retryable: false,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ALIBABA_PROVIDER;

    #[test]
    fn builds_region_specific_endpoint() {
        let profile = ApiProfile {
            id: "one".into(),
            name: "One".into(),
            provider: ALIBABA_PROVIDER.into(),
            region: Some("singapore".into()),
            workspace_id: Some("ws-example".into()),
            base_url: None,
            purpose: None,
            ..ApiProfile::default()
        };
        assert_eq!(
            base_url(&profile).unwrap(),
            "https://dashscope-intl.aliyuncs.com/compatible-mode/v1"
        );
    }
}
