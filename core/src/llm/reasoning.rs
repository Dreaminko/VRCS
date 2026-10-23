use serde_json::{json, Value};

use crate::config::ReasoningSetting;
use crate::providers::OpenAiProtocolBehavior;

use super::model_catalog::{ModelDetails, ReasoningControl, ReasoningOptions};
use super::{LlmError, LlmRequest};

fn control_error(code: &'static str, detail: impl Into<String>) -> LlmError {
    LlmError {
        code,
        detail: detail.into(),
        retryable: false,
        retry_after: None,
    }
}

fn current_field(details: &ModelDetails, field: &str) -> bool {
    !details.stale
        || details
            .field_sources
            .get(field)
            .or_else(|| details.field_sources.get("reasoning"))
            .is_some_and(|source| matches!(source.as_str(), "provider_api" | "builtin"))
}

/// Use only rules verified for the native endpoint. A compatible endpoint is not
/// the native provider, even when its model ID has the same spelling.
pub(super) fn builtin_options(provider: &str, model: &str) -> Option<ReasoningOptions> {
    let model = model.trim().trim_start_matches("models/");
    let efforts: &[&str] = match provider {
        "openai" => {
            let matches = |families: &[&str]| {
                families.iter().any(|family| {
                    model == *family
                        || model
                            .strip_prefix(*family)
                            .and_then(|suffix| suffix.strip_prefix('-'))
                            .is_some_and(|date| {
                                date.len() == 10
                                    && chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_ok()
                            })
                })
            };
            if matches(&["gpt-5", "gpt-5-mini", "gpt-5-nano"]) {
                &["minimal", "low", "medium", "high"]
            } else if matches(&["gpt-5.1"]) {
                &["none", "low", "medium", "high"]
            } else if matches(&[
                "gpt-5.2",
                "gpt-5.4",
                "gpt-5.4-mini",
                "gpt-5.4-nano",
                "gpt-5.5",
            ]) {
                &["none", "low", "medium", "high", "xhigh"]
            } else if matches(&["gpt-5.6", "gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna"]) {
                &["none", "low", "medium", "high", "xhigh", "max"]
            } else if matches(&["gpt-5.2-pro", "gpt-5.4-pro", "gpt-5.5-pro"]) {
                &["medium", "high", "xhigh"]
            } else {
                return None;
            }
        }
        "deepseek" => match model {
            "deepseek-flash"
            | "deepseek-v4-flash"
            | "deepseek-v4-flash-vision-exp"
            | "deepseek-v4-pro" => &["low", "high", "max"],
            _ => return None,
        },
        "mimo" if crate::providers::MIMO_CHAT_MODELS.contains(&model) => {
            return Some(ReasoningOptions {
                supported: Some(true),
                mandatory: Some(false),
                can_disable: Some(true),
                controls: vec![ReasoningControl::Toggle],
                ..Default::default()
            });
        }
        "groq" => match model {
            "openai/gpt-oss-20b" | "openai/gpt-oss-120b" => &["low", "medium", "high"],
            "qwen/qwen3.6-27b" => &["none", "default"],
            "qwen/qwen3.8-27b" => &["none", "default", "low", "medium", "high"],
            _ => return None,
        },
        "gemini" => {
            let budget = match model {
                "gemini-2.5-pro" => Some((128, 32768, false)),
                "gemini-2.5-flash" => Some((1, 24576, true)),
                "gemini-2.5-flash-lite" => Some((512, 24576, true)),
                _ => None,
            };
            if let Some((min, max, can_disable)) = budget {
                return Some(ReasoningOptions {
                    supported: Some(true),
                    mandatory: Some(!can_disable),
                    can_disable: Some(can_disable),
                    supports_dynamic_budget: Some(true),
                    controls: vec![ReasoningControl::BudgetTokens {
                        min: Some(min),
                        max: Some(max),
                    }],
                    ..Default::default()
                });
            }
            match model {
                "gemini-3-pro-preview" => &["low", "high"],
                "gemini-3.1-pro-preview" => &["low", "medium", "high"],
                "gemini-3-flash-preview" | "gemini-3.6-flash" | "gemini-3.5-flash-lite" => {
                    &["minimal", "low", "medium", "high"]
                }
                "gemini-3.8-flash" => &["low", "medium", "high"],
                _ => return None,
            }
        }
        _ => return None,
    };
    let can_disable = provider == "deepseek" || efforts.contains(&"none");
    let values = efforts
        .iter()
        .map(|value| (*value).to_owned())
        .collect::<Vec<_>>();
    Some(ReasoningOptions {
        supported: Some(true),
        mandatory: Some(!can_disable),
        can_disable: Some(can_disable),
        supported_efforts: Some(values.clone()),
        controls: vec![ReasoningControl::Effort { values }],
        ..Default::default()
    })
}

pub(super) fn add_builtin_options(details: &mut ModelDetails, provider: &str, model: &str) {
    let Some(fallback) = builtin_options(provider, model) else {
        return;
    };
    let public_field = |field: &str| {
        details
            .field_sources
            .get(field)
            .or_else(|| details.field_sources.get("reasoning"))
            .is_some_and(|source| source == "models_dev")
    };
    if details
        .reasoning
        .as_ref()
        .is_some_and(|options| options.supported == Some(false))
        && current_field(details, "reasoning")
        && !public_field("reasoning")
    {
        return;
    }
    let fields = [
        "reasoning",
        "reasoning.controls",
        "reasoning.supported_efforts",
        "reasoning.can_disable",
        "reasoning.supports_dynamic_budget",
    ];
    let current = fields.map(|field| current_field(details, field) && !public_field(field));
    let options = details
        .reasoning
        .get_or_insert_with(ReasoningOptions::default);
    if !current[0] || options.supported.is_none() {
        options.supported = fallback.supported;
    }
    if !current[0] || options.mandatory.is_none() {
        options.mandatory = fallback.mandatory;
        details
            .field_sources
            .insert("reasoning.mandatory".into(), "builtin".into());
    }
    for (index, missing) in [
        (1, options.controls.is_empty()),
        (2, options.supported_efforts.is_none()),
        (3, options.can_disable.is_none()),
        (4, options.supports_dynamic_budget.is_none()),
    ] {
        if !current[index] || missing {
            match index {
                1 => options.controls.clone_from(&fallback.controls),
                2 => options
                    .supported_efforts
                    .clone_from(&fallback.supported_efforts),
                3 => options.can_disable = fallback.can_disable,
                4 => options.supports_dynamic_budget = fallback.supports_dynamic_budget,
                _ => unreachable!(),
            }
            details
                .field_sources
                .insert(fields[index].into(), "builtin".into());
        }
    }
    if !current[0] || !details.field_sources.contains_key("reasoning") {
        details
            .field_sources
            .insert("reasoning".into(), "builtin".into());
    }
}

/// Apply explicit settings once, after legacy mapping and output-cap constraints.
/// An absent setting deliberately leaves the existing request unchanged.
/// Returns whether an explicit setting was active for this request.
pub(super) fn apply_explicit_control(
    body: &mut Value,
    provider: &str,
    request: &LlmRequest<'_>,
    details: Option<&ModelDetails>,
) -> Result<bool, LlmError> {
    let Some(setting) = request.reasoning_control.as_ref() else {
        return Ok(false);
    };
    if !matches!(
        provider,
        "openai"
            | "gemini"
            | "deepseek"
            | "groq"
            | "alibaba_cloud"
            | "alibaba_token_plan"
            | "openrouter"
    ) {
        return Err(control_error(
            "llm.reasoning_unsupported",
            "This endpoint has no verified reasoning adjustment protocol",
        ));
    }
    let mut details = details.cloned().unwrap_or_default();
    add_builtin_options(&mut details, provider, request.model);
    if matches!(provider, "alibaba_cloud" | "alibaba_token_plan") {
        let control = alibaba_thinking_control(request.model);
        if let Some(options) = &mut details.reasoning {
            if options.mandatory.is_none() && control == ThinkingControl::HideOnly {
                options.mandatory = Some(true);
            }
        }
    }
    let options = details.reasoning.as_ref().ok_or_else(|| {
        control_error(
            "llm.reasoning_capability_unavailable",
            "Reasoning controls are not confirmed for this model",
        )
    })?;
    if !current_field(&details, "reasoning") {
        return Err(control_error(
            "llm.reasoning_capability_unavailable",
            "Reasoning metadata is out of date",
        ));
    }
    if options.supported == Some(false) {
        return Err(control_error(
            "llm.reasoning_unsupported",
            "This model does not support reasoning controls",
        ));
    }
    let can_disable =
        current_field(&details, "reasoning.can_disable") && options.can_disable == Some(true);
    let has_toggle = current_field(&details, "reasoning.controls")
        && options
            .controls
            .iter()
            .any(|control| matches!(control, ReasoningControl::Toggle));
    if !request.thinking_enabled && options.mandatory != Some(true) && (can_disable || has_toggle) {
        return Ok(false);
    }
    if provider == "gemini" {
        if let Some(limit) = body.pointer_mut("/generationConfig/maxOutputTokens") {
            if let Some(current) = limit.as_u64() {
                *limit = json!(current.min(u64::from(request.max_output_tokens)));
            }
        }
    }
    setting.validate().map_err(|detail| {
        control_error(
            match setting {
                ReasoningSetting::BudgetTokens(_) => "llm.reasoning_budget_invalid",
                _ => "llm.reasoning_unsupported",
            },
            detail,
        )
    })?;
    match setting {
        ReasoningSetting::Effort(effort) => {
            if !current_field(&details, "reasoning.supported_efforts")
                || (provider == "openrouter"
                    && details
                        .field_sources
                        .get("reasoning.supported_efforts")
                        .or_else(|| details.field_sources.get("reasoning"))
                        .is_none_or(|source| source != "provider_api"))
            {
                return Err(control_error(
                    "llm.reasoning_capability_unavailable",
                    "Reasoning effort metadata is out of date",
                ));
            }
            let supported = options.supported_efforts.as_ref().or_else(|| {
                options.controls.iter().find_map(|control| match control {
                    ReasoningControl::Effort { values }
                        if current_field(&details, "reasoning.controls") =>
                    {
                        Some(values)
                    }
                    _ => None,
                })
            });
            if effort == "none" || !supported.is_some_and(|values| values.contains(effort)) {
                return Err(control_error(
                    "llm.reasoning_unsupported",
                    format!("Reasoning effort {effort} is not supported by this model"),
                ));
            }
            // Public metadata cannot broaden the native protocol's verified values.
            if builtin_options(provider, request.model).is_some_and(|native| {
                native
                    .supported_efforts
                    .is_none_or(|values| !values.contains(effort))
            }) {
                return Err(control_error(
                    "llm.reasoning_unsupported",
                    "This effort is not supported by the native model protocol",
                ));
            }
            match provider {
                "openai" => body["reasoning"] = json!({"effort": effort}),
                "gemini" => {
                    body["generationConfig"]["thinkingConfig"] = json!({"thinkingLevel": effort})
                }
                "openrouter" => body["reasoning"] = json!({"effort": effort, "exclude": true}),
                _ => {
                    body["reasoning_effort"] = json!(effort);
                }
            }
        }
        ReasoningSetting::BudgetTokens(budget) => {
            if !matches!(
                provider,
                "gemini" | "alibaba_cloud" | "alibaba_token_plan" | "openrouter"
            ) {
                return Err(control_error(
                    "llm.reasoning_unsupported",
                    "This endpoint does not support a thinking token budget",
                ));
            }
            if !current_field(&details, "reasoning.controls")
                || (provider == "openrouter"
                    && !current_field(&details, "reasoning.supports_max_tokens"))
            {
                return Err(control_error(
                    "llm.reasoning_capability_unavailable",
                    "Reasoning budget metadata is out of date",
                ));
            }
            let bounds = options.controls.iter().find_map(|control| match control {
                ReasoningControl::BudgetTokens { min, max } => Some((*min, *max)),
                _ => None,
            });
            let bounds = if provider == "openrouter" && options.supports_max_tokens == Some(true) {
                bounds.or(Some((Some(1), None)))
            } else {
                bounds
            };
            let (min, max) = bounds.ok_or_else(|| {
                control_error(
                    "llm.reasoning_unsupported",
                    "This model has no confirmed reasoning budget control",
                )
            })?;
            if provider == "openrouter" && options.supports_max_tokens != Some(true) {
                return Err(control_error(
                    "llm.reasoning_unsupported",
                    "The gateway does not confirm token budget support",
                ));
            }
            if provider == "openrouter"
                && details
                    .field_sources
                    .get("reasoning.supports_max_tokens")
                    .or_else(|| details.field_sources.get("reasoning"))
                    .is_none_or(|source| source != "provider_api")
            {
                return Err(control_error(
                    "llm.reasoning_capability_unavailable",
                    "The gateway token-budget declaration is unavailable",
                ));
            }
            let dynamic = *budget == -1
                && provider == "gemini"
                && current_field(&details, "reasoning.supports_dynamic_budget")
                && options.supports_dynamic_budget == Some(true);
            let native_bounds = builtin_options(provider, request.model).and_then(|native| {
                native
                    .controls
                    .into_iter()
                    .find_map(|control| match control {
                        ReasoningControl::BudgetTokens { min, max } => Some((min, max)),
                        _ => None,
                    })
            });
            let in_range = |(min, max): (Option<i64>, Option<u32>)| {
                *budget > 0
                    && min.is_none_or(|min| *budget >= min.max(1))
                    && max.is_none_or(|max| *budget <= i64::from(max))
            };
            if !dynamic
                && (!in_range((min, max)) || native_bounds.is_some_and(|bounds| !in_range(bounds)))
            {
                return Err(control_error(
                    "llm.reasoning_budget_invalid",
                    "Thinking budget is outside the supported model range",
                ));
            }
            let limit = if provider == "gemini" {
                body.pointer("/generationConfig/maxOutputTokens")
            } else {
                body.get("max_tokens")
            };
            if !dynamic
                && limit
                    .and_then(Value::as_i64)
                    .is_none_or(|limit| *budget >= limit)
            {
                return Err(control_error(
                    "llm.reasoning_budget_invalid",
                    "Thinking budget must be smaller than the final total output token limit",
                ));
            }
            match provider {
                "gemini" => {
                    body["generationConfig"]["thinkingConfig"] = json!({"thinkingBudget": budget})
                }
                "openrouter" => body["reasoning"] = json!({"max_tokens": budget, "exclude": true}),
                _ => {
                    body["thinking_budget"] = json!(budget);
                }
            }
        }
    }
    if let Some(fields) = body.as_object_mut() {
        match provider {
            "deepseek" => {
                fields.remove("temperature");
                fields.insert("thinking".into(), json!({"type": "enabled"}));
            }
            "alibaba_cloud" | "alibaba_token_plan" => {
                if options.mandatory == Some(true) {
                    fields.remove("enable_thinking");
                } else if has_toggle || can_disable {
                    fields.insert("enable_thinking".into(), json!(true));
                }
                match setting {
                    ReasoningSetting::Effort(_) => {
                        fields.remove("thinking_budget");
                    }
                    ReasoningSetting::BudgetTokens(_) => {
                        fields.remove("reasoning_effort");
                    }
                }
            }
            "openrouter" => {
                fields.remove("reasoning_effort");
            }
            _ => {}
        }
    }
    Ok(true)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ThinkingControl {
    Unsupported,
    HideOnly,
    DisableSupported,
}

pub(super) fn thinking_control(behavior: OpenAiProtocolBehavior, model: &str) -> ThinkingControl {
    match behavior {
        OpenAiProtocolBehavior::DeepSeek => ThinkingControl::DisableSupported,
        OpenAiProtocolBehavior::Groq => groq_thinking_control(model),
        OpenAiProtocolBehavior::Alibaba => alibaba_thinking_control(model),
        OpenAiProtocolBehavior::Mimo => {
            if crate::providers::MIMO_CHAT_MODELS.contains(&model) {
                ThinkingControl::DisableSupported
            } else {
                ThinkingControl::Unsupported
            }
        }
        OpenAiProtocolBehavior::Standard => ThinkingControl::Unsupported,
    }
}

pub(super) fn apply_chat_completion_reasoning(
    body: &mut Value,
    behavior: OpenAiProtocolBehavior,
    model: &str,
    enabled: bool,
    stream: bool,
) {
    match behavior {
        OpenAiProtocolBehavior::DeepSeek => {
            body["thinking"] = json!({
                "type": if enabled { "enabled" } else { "disabled" }
            });
            if !enabled {
                body["temperature"] = json!(0);
            }
            if stream {
                body["stream_options"] = json!({ "include_usage": true });
            }
        }
        OpenAiProtocolBehavior::Mimo => {
            if thinking_control(behavior, model) == ThinkingControl::DisableSupported {
                body["thinking"] = json!({
                    "type": if enabled { "enabled" } else { "disabled" }
                });
            }
        }
        OpenAiProtocolBehavior::Groq => apply_groq_reasoning(body, model, enabled),
        OpenAiProtocolBehavior::Alibaba => {
            if thinking_control(behavior, model) == ThinkingControl::DisableSupported {
                body["enable_thinking"] = json!(enabled);
            }
        }
        OpenAiProtocolBehavior::Standard => {}
    }
}

pub(super) fn should_filter_inline_reasoning(
    behavior: OpenAiProtocolBehavior,
    model: &str,
) -> bool {
    behavior == OpenAiProtocolBehavior::Groq
        && matches!(
            normalized_model(model),
            "qwen/qwen3.6-27b" | "qwen/qwen3.8-27b" | "minimaxai/minimax-m2.7"
        )
}

pub(super) fn sanitize_response_text(
    behavior: OpenAiProtocolBehavior,
    model: &str,
    text: &str,
) -> String {
    if !should_filter_inline_reasoning(behavior, model) {
        return text.trim().to_owned();
    }
    let mut filter = InlineReasoningFilter::default();
    let mut output = filter.push(text);
    output.push_str(&filter.finish());
    output.trim().to_owned()
}

fn apply_groq_reasoning(body: &mut Value, model: &str, enabled: bool) {
    match normalized_model(model) {
        "openai/gpt-oss-20b" | "openai/gpt-oss-120b" | "openai/gpt-oss-safeguard-20b" => {
            body["include_reasoning"] = json!(false);
        }
        "qwen/qwen3.6-27b" => {
            body["reasoning_format"] = json!("hidden");
            body["reasoning_effort"] = json!(if enabled { "default" } else { "none" });
        }
        "qwen/qwen3.8-27b" => {
            body["reasoning_format"] = json!("hidden");
            body["reasoning_effort"] = json!(if enabled { "medium" } else { "none" });
        }
        "minimaxai/minimax-m2.7" => {
            body["reasoning_format"] = json!("hidden");
        }
        _ => {}
    }
}

fn groq_thinking_control(model: &str) -> ThinkingControl {
    match normalized_model(model) {
        "qwen/qwen3.6-27b" | "qwen/qwen3.8-27b" => ThinkingControl::DisableSupported,
        "openai/gpt-oss-20b"
        | "openai/gpt-oss-120b"
        | "openai/gpt-oss-safeguard-20b"
        | "minimaxai/minimax-m2.7" => ThinkingControl::HideOnly,
        _ => ThinkingControl::Unsupported,
    }
}

fn alibaba_thinking_control(model: &str) -> ThinkingControl {
    let model = normalized_model(model);
    if is_alibaba_thinking_only_model(model) {
        ThinkingControl::HideOnly
    } else if is_alibaba_hybrid_model(model) {
        ThinkingControl::DisableSupported
    } else {
        ThinkingControl::Unsupported
    }
}

fn is_alibaba_thinking_only_model(model: &str) -> bool {
    (model.starts_with("qwen3-vl-") && model.contains("-thinking"))
        || model == "qwq-plus"
        || model == "qwen3.7-max-preview"
        || model == "qwen3.7-max-2026-05-17"
        || model == "qwen3-next-80b-a3b-thinking"
        || model.starts_with("qwen3-235b-a22b-thinking-")
        || model.starts_with("qwen3-30b-a3b-thinking-")
        || model == "deepseek-r1"
        || model.starts_with("deepseek-r1-")
        || model.starts_with("siliconflow/deepseek-r1")
        || model.starts_with("vanchin/deepseek-r1")
        || model == "kimi-k2.7-code"
        || model == "kimi-k2-thinking"
        || model.starts_with("kimi/kimi-k2.7-code")
        || matches!(
            model,
            "MiniMax-M2.5" | "MiniMax-M2.1" | "minimax-m2.5" | "minimax-m2.1"
        )
}

fn is_alibaba_hybrid_model(model: &str) -> bool {
    model.starts_with("qwen3.8-max")
        || model.starts_with("qwen3.8-flash")
        || model.starts_with("qwen3-vl-plus")
        || model.starts_with("qwen3.7-max")
        || model.starts_with("qwen3.7-plus")
        || model == "qwen3.6-max-preview"
        || model == "qwen3.6-35b-a3b"
        || model.starts_with("qwen3.6-plus")
        || model.starts_with("qwen3.6-flash")
        || model.starts_with("qwen3.5-plus")
        || model.starts_with("qwen3.5-flash")
        || matches!(
            model,
            "qwen3.5-397b-a17b"
                | "qwen3.5-122b-a10b"
                | "qwen3.5-27b"
                | "qwen3.5-35b-a3b"
                | "qwen3-235b-a22b"
                | "qwen3-32b"
                | "qwen3-30b-a3b"
                | "qwen3-14b"
                | "qwen3-8b"
        )
        || model.starts_with("qwen3-max")
        || model.starts_with("qwen-plus")
        || model.starts_with("qwen-flash")
        || model.starts_with("qwen-turbo")
        || model.starts_with("deepseek-v4-")
        || model.starts_with("deepseek-v4.1-")
        || model.starts_with("vanchin/deepseek-v4-")
        || model.starts_with("vanchin/deepseek-v4.1-")
        || model.starts_with("deepseek-v3.2")
        || model.starts_with("deepseek-v3.1")
        || model.starts_with("siliconflow/deepseek-v3.")
        || model.starts_with("vanchin/deepseek-v3.")
        || matches!(
            model,
            "glm-5.2"
                | "glm-5.2-us"
                | "glm-5.2-fast-preview"
                | "glm-5.1"
                | "glm-5"
                | "glm-4.7"
                | "glm-4.6"
                | "glm-4.5"
                | "glm-4.5-air"
        )
        || matches!(
            model,
            "kimi-k2.6" | "kimi-k2.5" | "kimi/kimi-k2.6" | "kimi/kimi-k2.5"
        )
}

fn normalized_model(model: &str) -> &str {
    model.trim()
}

#[derive(Debug, Default)]
pub(super) struct InlineReasoningFilter {
    inside_think: bool,
    pending: String,
}

impl InlineReasoningFilter {
    pub(super) fn push(&mut self, text: &str) -> String {
        const OPEN: &str = "<think>";
        const CLOSE: &str = "</think>";

        self.pending.push_str(text);
        let mut visible = String::new();
        loop {
            let tag = if self.inside_think { CLOSE } else { OPEN };
            if let Some(position) = self.pending.find(tag) {
                if !self.inside_think {
                    visible.push_str(&self.pending[..position]);
                }
                self.pending.drain(..position + tag.len());
                self.inside_think = !self.inside_think;
                continue;
            }

            let keep = partial_tag_suffix_len(&self.pending, tag);
            if self.inside_think {
                if keep == 0 {
                    self.pending.clear();
                } else {
                    self.pending.drain(..self.pending.len() - keep);
                }
            } else {
                let visible_end = self.pending.len() - keep;
                visible.push_str(&self.pending[..visible_end]);
                self.pending.drain(..visible_end);
            }
            return visible;
        }
    }

    pub(super) fn finish(&mut self) -> String {
        if self.inside_think {
            self.pending.clear();
            return String::new();
        }
        std::mem::take(&mut self.pending)
    }
}

fn partial_tag_suffix_len(text: &str, tag: &str) -> usize {
    (1..tag.len())
        .rev()
        .find(|length| text.ends_with(&tag[..*length]))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mimo_reasoning_exposes_only_verified_toggles() {
        for model in [
            "mimo-v2.6-flash",
            "mimo-v2.6-pro",
            "mimo-v2.6-pro-ultraspeed",
        ] {
            let options = builtin_options("mimo", model).unwrap();
            assert_eq!(options.can_disable, Some(true));
            assert_eq!(options.mandatory, Some(false));
            assert!(matches!(
                options.controls.as_slice(),
                [ReasoningControl::Toggle]
            ));
            assert!(options.supported_efforts.is_none());
        }
        for model in ["mimo-v2.5-asr", "mimo-v2.5", "mimo-future"] {
            assert!(builtin_options("mimo", model).is_none());
        }
        assert!(builtin_options("openai_compatible", "mimo-v2.6-flash").is_none());
    }

    #[test]
    fn deepseek_alias_efforts_work_without_catalog_metadata_and_preserve_the_toggle() {
        for model in [
            "deepseek-flash",
            "deepseek-v4-flash",
            "deepseek-v4-flash-vision-exp",
            "deepseek-v4-pro",
        ] {
            for effort in ["low", "high", "max"] {
                let mut request = LlmRequest {
                    model,
                    instructions: "Answer",
                    input: "Hello",
                    max_output_tokens: 1024,
                    thinking_enabled: true,
                    reasoning_control: Some(ReasoningSetting::Effort(effort.into())),
                };
                let mut body = json!({"max_tokens": 1024, "temperature": 0.7});
                assert!(apply_explicit_control(&mut body, "deepseek", &request, None).unwrap());
                assert_eq!(body["reasoning_effort"], effort, "{model}");
                assert_eq!(body["thinking"]["type"], "enabled");
                assert!(body.get("temperature").is_none());

                request.thinking_enabled = false;
                let original = json!({"max_tokens": 1024, "thinking": {"type": "disabled"}});
                let mut body = original.clone();
                assert!(!apply_explicit_control(&mut body, "deepseek", &request, None).unwrap());
                assert_eq!(body, original, "{model}");

                request.thinking_enabled = true;
                request.reasoning_control = Some(ReasoningSetting::Effort("medium".into()));
                assert_eq!(
                    apply_explicit_control(&mut body, "deepseek", &request, None)
                        .unwrap_err()
                        .code,
                    "llm.reasoning_unsupported"
                );
            }
        }
    }

    #[test]
    fn explicit_controls_use_native_fields_and_keep_mandatory_reasoning_active() {
        use crate::config::ReasoningSetting;
        for (provider, model, setting, pointer, expected) in [
            (
                "openai",
                "gpt-5-mini",
                ReasoningSetting::Effort("high".into()),
                "/reasoning/effort",
                json!("high"),
            ),
            (
                "gemini",
                "gemini-2.5-pro",
                ReasoningSetting::BudgetTokens(128),
                "/generationConfig/thinkingConfig/thinkingBudget",
                json!(128),
            ),
            (
                "groq",
                "openai/gpt-oss-120b",
                ReasoningSetting::Effort("low".into()),
                "/reasoning_effort",
                json!("low"),
            ),
        ] {
            let request = super::super::LlmRequest {
                model,
                instructions: "Answer",
                input: "Hello",
                max_output_tokens: 1024,
                thinking_enabled: false,
                reasoning_control: Some(setting),
            };
            let mut body = json!({"max_output_tokens": 1024, "max_tokens": 1024, "generationConfig": {"maxOutputTokens": 1024}});
            apply_explicit_control(&mut body, provider, &request, None).unwrap();
            assert_eq!(body.pointer(pointer), Some(&expected), "{provider}");
        }
    }

    #[test]
    fn verified_fallback_survives_stale_public_fields_without_inventing_models() {
        let stale = ModelDetails {
            stale: true,
            reasoning: Some(ReasoningOptions {
                supported: Some(true),
                supported_efforts: Some(vec!["invented".into()]),
                ..Default::default()
            }),
            field_sources: [("reasoning".into(), "models_dev".into())].into(),
            ..Default::default()
        };
        let mut request = LlmRequest {
            model: "gpt-5-mini-2025-08-07",
            instructions: "Answer",
            input: "Hello",
            max_output_tokens: 1024,
            thinking_enabled: false,
            reasoning_control: Some(ReasoningSetting::Effort("high".into())),
        };
        let mut body = json!({"max_output_tokens": 1024});
        apply_explicit_control(&mut body, "openai", &request, Some(&stale)).unwrap();
        assert_eq!(body["reasoning"]["effort"], "high");
        request.model = "gpt-5-2099-custom";
        assert_eq!(
            apply_explicit_control(&mut body, "openai", &request, Some(&stale))
                .unwrap_err()
                .code,
            "llm.reasoning_capability_unavailable"
        );
        request.model = "gemini-2.5-pro";
        request.reasoning_control = Some(ReasoningSetting::BudgetTokens(128));
        let mut body = json!({"generationConfig": {"maxOutputTokens": 2048}});
        apply_explicit_control(&mut body, "gemini", &request, Some(&stale)).unwrap();
        assert_eq!(body["generationConfig"]["maxOutputTokens"], 1024);
    }

    #[test]
    fn gateway_missing_fields_cannot_inherit_explicit_controls_from_public_metadata() {
        let mut catalog = super::super::openrouter::parse_catalog(&json!({"data": [{
            "id": "gateway-model", "architecture": {"output_modalities": ["text"]},
            "reasoning": {"mandatory": true}
        }]}));
        let details = catalog.model_details.get_mut("gateway-model").unwrap();
        details.supplement(&ModelDetails {
            reasoning: Some(ReasoningOptions {
                supported: Some(true),
                supported_efforts: Some(vec!["high".into()]),
                controls: vec![ReasoningControl::BudgetTokens {
                    min: Some(1),
                    max: Some(1024),
                }],
                supports_max_tokens: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        });
        let mut request = LlmRequest {
            model: "gateway-model",
            instructions: "Answer",
            input: "Hello",
            max_output_tokens: 1024,
            thinking_enabled: false,
            reasoning_control: Some(ReasoningSetting::Effort("high".into())),
        };
        let mut body = json!({"max_tokens": 1024});
        assert_eq!(
            apply_explicit_control(&mut body, "openrouter", &request, Some(details))
                .unwrap_err()
                .code,
            "llm.reasoning_capability_unavailable"
        );
        request.reasoning_control = Some(ReasoningSetting::BudgetTokens(128));
        assert_eq!(
            apply_explicit_control(&mut body, "openrouter", &request, Some(details))
                .unwrap_err()
                .code,
            "llm.reasoning_unsupported"
        );
    }

    #[test]
    fn explicit_settings_are_ignored_only_when_a_hybrid_model_is_disabled() {
        let request = super::super::LlmRequest {
            model: "qwen/qwen3.8-27b",
            instructions: "Answer",
            input: "Hello",
            max_output_tokens: 1024,
            thinking_enabled: false,
            reasoning_control: Some(crate::config::ReasoningSetting::Effort("invalid".into())),
        };
        let original = json!({"max_tokens": 1024, "reasoning_effort": "none"});
        let mut body = original.clone();
        apply_explicit_control(&mut body, "groq", &request, None).unwrap();
        assert_eq!(body, original);
        assert_eq!(
            apply_explicit_control(&mut body, "openai_compatible", &request, None)
                .unwrap_err()
                .code,
            "llm.reasoning_unsupported"
        );
    }

    #[test]
    fn explicit_budget_validates_model_range_dynamic_mode_and_final_total_limit() {
        let mut request = super::super::LlmRequest {
            model: "gemini-2.5-flash-lite",
            instructions: "Answer",
            input: "Hello",
            max_output_tokens: 1024,
            thinking_enabled: true,
            reasoning_control: Some(crate::config::ReasoningSetting::BudgetTokens(128)),
        };
        let mut body = json!({"generationConfig": {"maxOutputTokens": 1024}});
        assert_eq!(
            apply_explicit_control(&mut body, "gemini", &request, None)
                .unwrap_err()
                .code,
            "llm.reasoning_budget_invalid"
        );
        request.reasoning_control = Some(crate::config::ReasoningSetting::BudgetTokens(-1));
        apply_explicit_control(&mut body, "gemini", &request, None).unwrap();
        assert_eq!(
            body.pointer("/generationConfig/thinkingConfig/thinkingBudget"),
            Some(&json!(-1))
        );
        request.reasoning_control = Some(crate::config::ReasoningSetting::BudgetTokens(512));
        body["generationConfig"]["maxOutputTokens"] = json!(512);
        assert_eq!(
            apply_explicit_control(&mut body, "gemini", &request, None)
                .unwrap_err()
                .code,
            "llm.reasoning_budget_invalid"
        );
        body["generationConfig"]["maxOutputTokens"] = json!(513);
        apply_explicit_control(&mut body, "gemini", &request, None).unwrap();
        assert_eq!(body["generationConfig"]["maxOutputTokens"], 513);
    }

    #[test]
    fn explicit_protocol_mapping_requires_confirmed_metadata_and_preserves_user_effort() {
        use super::super::model_catalog::{ModelDetails, ReasoningControl, ReasoningOptions};
        let details = ModelDetails {
            reasoning: Some(ReasoningOptions {
                supported: Some(true),
                mandatory: Some(false),
                can_disable: Some(true),
                supported_efforts: Some(vec!["low".into(), "high".into()]),
                controls: vec![ReasoningControl::BudgetTokens {
                    min: Some(128),
                    max: Some(2048),
                }],
                supports_max_tokens: Some(true),
                ..Default::default()
            }),
            field_sources: [("reasoning".into(), "provider_api".into())].into(),
            ..Default::default()
        };
        let mut request = super::super::LlmRequest {
            model: "verified-model",
            instructions: "Answer",
            input: "Hello",
            max_output_tokens: 1024,
            thinking_enabled: true,
            reasoning_control: Some(crate::config::ReasoningSetting::Effort("high".into())),
        };
        for (provider, pointer) in [
            ("deepseek", "/reasoning_effort"),
            ("alibaba_cloud", "/reasoning_effort"),
            ("openrouter", "/reasoning/effort"),
        ] {
            let mut body = json!({"max_tokens": 1024});
            details.apply_request(&mut body, provider, true);
            apply_explicit_control(&mut body, provider, &request, Some(&details)).unwrap();
            assert_eq!(body.pointer(pointer), Some(&json!("high")));
        }
        request.reasoning_control = Some(crate::config::ReasoningSetting::BudgetTokens(256));
        for (provider, pointer) in [
            ("alibaba_cloud", "/thinking_budget"),
            ("openrouter", "/reasoning/max_tokens"),
        ] {
            let mut body = json!({"max_tokens": 1024, "reasoning_effort": "low"});
            apply_explicit_control(&mut body, provider, &request, Some(&details)).unwrap();
            assert_eq!(body.pointer(pointer), Some(&json!(256)));
            assert!(body.get("reasoning_effort").is_none());
        }
        let stale = ModelDetails {
            stale: true,
            field_sources: [
                ("reasoning".into(), "provider_api".into()),
                ("reasoning.controls".into(), "models_dev".into()),
            ]
            .into(),
            ..details.clone()
        };
        let mut body = json!({"max_tokens": 1024});
        assert_eq!(
            apply_explicit_control(&mut body, "alibaba_cloud", &request, Some(&stale))
                .unwrap_err()
                .code,
            "llm.reasoning_capability_unavailable"
        );
        assert_eq!(
            apply_explicit_control(&mut body, "qwen_ai", &request, Some(&details))
                .unwrap_err()
                .code,
            "llm.reasoning_unsupported"
        );
    }

    #[test]
    fn classifies_supported_thinking_controls() {
        assert_eq!(
            thinking_control(OpenAiProtocolBehavior::Groq, "qwen/qwen3.6-27b"),
            ThinkingControl::DisableSupported
        );
        assert_eq!(
            thinking_control(OpenAiProtocolBehavior::Groq, "openai/gpt-oss-120b"),
            ThinkingControl::HideOnly
        );
        assert_eq!(
            thinking_control(OpenAiProtocolBehavior::Alibaba, "qwen-plus"),
            ThinkingControl::DisableSupported
        );
        assert_eq!(
            thinking_control(OpenAiProtocolBehavior::Alibaba, "qwen3.8-max"),
            ThinkingControl::DisableSupported
        );
        assert_eq!(
            thinking_control(OpenAiProtocolBehavior::Alibaba, "deepseek-r1"),
            ThinkingControl::HideOnly
        );
        assert_eq!(
            thinking_control(OpenAiProtocolBehavior::Standard, "qwen/qwen3.6-27b"),
            ThinkingControl::Unsupported
        );
    }

    #[test]
    fn qwen_vision_thinking_controls_preserve_fixed_mode_models() {
        for (model, expected) in [
            ("qwen3.8-flash", ThinkingControl::DisableSupported),
            ("qwen3-vl-plus", ThinkingControl::DisableSupported),
            ("qwen3-vl-8b-thinking", ThinkingControl::HideOnly),
            ("qwen3-vl-8b-instruct", ThinkingControl::Unsupported),
        ] {
            assert_eq!(
                thinking_control(OpenAiProtocolBehavior::Alibaba, model),
                expected
            );
            for enabled in [false, true] {
                let mut body = json!({});
                apply_chat_completion_reasoning(
                    &mut body,
                    OpenAiProtocolBehavior::Alibaba,
                    model,
                    enabled,
                    false,
                );
                let toggle =
                    (expected == ThinkingControl::DisableSupported).then(|| json!(enabled));
                assert_eq!(body.get("enable_thinking"), toggle.as_ref(), "{model}");
            }
        }
    }

    #[test]
    fn removes_complete_inline_thinking() {
        assert_eq!(
            sanitize_response_text(
                OpenAiProtocolBehavior::Groq,
                "qwen/qwen3.6-27b",
                "<think>private reasoning</think>Visible answer"
            ),
            "Visible answer"
        );
    }

    #[test]
    fn removes_inline_thinking_when_tags_cross_chunks() {
        let mut filter = InlineReasoningFilter::default();
        assert_eq!(filter.push("<thi"), "");
        assert_eq!(filter.push("nk>private"), "");
        assert_eq!(filter.push(" reasoning</thi"), "");
        assert_eq!(filter.push("nk>Visible"), "Visible");
        assert_eq!(filter.push(" answer"), " answer");
        assert_eq!(filter.finish(), "");
    }
}
