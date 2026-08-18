use std::time::Duration;

use crate::config::ApiProfile;
use crate::providers::OpenAiProtocolBehavior;

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
        OpenAiProtocolBehavior::Alibaba,
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

fn base_url(profile: &ApiProfile) -> Result<String, LlmError> {
    let workspace = profile.workspace_id.as_deref().unwrap_or("").trim();
    match (profile.region.as_deref(), workspace.is_empty()) {
        (Some("china_beijing"), false) => Ok(format!(
            "https://{workspace}.cn-beijing.maas.aliyuncs.com/compatible-mode/v1"
        )),
        (Some("singapore"), false) => Ok(format!(
            "https://{workspace}.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1"
        )),
        (Some("china_beijing"), true) => {
            Ok("https://dashscope.aliyuncs.com/compatible-mode/v1".into())
        }
        (Some("singapore"), true) => {
            Ok("https://dashscope-intl.aliyuncs.com/compatible-mode/v1".into())
        }
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
    use crate::providers::ALIBABA_PROVIDER;

    #[test]
    fn builds_workspace_endpoints_for_supported_regions() {
        let mut profile = ApiProfile {
            id: "one".into(),
            name: "One".into(),
            provider: ALIBABA_PROVIDER.into(),
            region: Some("singapore".into()),
            workspace_id: Some("ws-example".into()),
            base_url: None,
            ..ApiProfile::default()
        };
        assert_eq!(
            base_url(&profile).unwrap(),
            "https://ws-example.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1"
        );

        profile.region = Some("china_beijing".into());
        assert_eq!(
            base_url(&profile).unwrap(),
            "https://ws-example.cn-beijing.maas.aliyuncs.com/compatible-mode/v1"
        );
    }

    #[test]
    fn keeps_legacy_global_endpoint_without_a_workspace() {
        let profile = ApiProfile {
            provider: ALIBABA_PROVIDER.into(),
            region: Some("singapore".into()),
            ..ApiProfile::default()
        };

        assert_eq!(
            base_url(&profile).unwrap(),
            "https://dashscope-intl.aliyuncs.com/compatible-mode/v1"
        );
    }
}
