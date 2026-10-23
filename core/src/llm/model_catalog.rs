use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelDetails {
    pub input_modalities: Vec<String>,
    pub output_modalities: Vec<String>,
    pub context_tokens: Option<u32>,
    pub max_output_tokens: Option<u32>,
    pub reasoning: Option<ReasoningOptions>,
    pub temperature: Option<bool>,
    pub structured_output: Option<bool>,
    pub tool_call: Option<bool>,
    pub status: Option<String>,
    pub canonical_model_id: Option<String>,
    pub field_sources: BTreeMap<String, String>,
    pub updated_at: Option<i64>,
    pub stale: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ReasoningOptions {
    pub mandatory: Option<bool>,
    pub can_disable: Option<bool>,
    pub supports_max_tokens: Option<bool>,
    pub supports_dynamic_budget: Option<bool>,
    pub supported_efforts: Option<Vec<String>>,
    pub default_effort: Option<String>,
    pub supported: Option<bool>,
    pub controls: Vec<ReasoningControl>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ReasoningControl {
    Toggle,
    Effort { values: Vec<String> },
    BudgetTokens { min: Option<i64>, max: Option<u32> },
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ModelCatalog {
    pub models: Vec<String>,
    #[serde(default)]
    pub model_details: BTreeMap<String, ModelDetails>,
}

impl ModelDetails {
    /// Fill only missing provider fields. A provider's explicit value always wins.
    pub fn supplement(&mut self, fallback: &Self) {
        macro_rules! fill {
            ($field:ident) => {
                if self.$field.is_none() {
                    self.$field.clone_from(&fallback.$field);
                    if self.$field.is_some() {
                        self.field_sources
                            .insert(stringify!($field).into(), "models_dev".into());
                    }
                }
            };
        }
        for (name, target, source) in [
            (
                "input_modalities",
                &mut self.input_modalities,
                &fallback.input_modalities,
            ),
            (
                "output_modalities",
                &mut self.output_modalities,
                &fallback.output_modalities,
            ),
        ] {
            if target.is_empty() && !source.is_empty() {
                target.clone_from(source);
                self.field_sources.insert(name.into(), "models_dev".into());
            }
        }
        fill!(context_tokens);
        fill!(max_output_tokens);
        fill!(temperature);
        fill!(structured_output);
        fill!(tool_call);
        fill!(status);
        fill!(canonical_model_id);
        if self.reasoning.is_none() {
            self.reasoning = fallback.reasoning.clone();
            if self.reasoning.is_some() {
                self.field_sources
                    .insert("reasoning".into(), "models_dev".into());
            }
        } else if let (Some(target), Some(source)) = (&mut self.reasoning, &fallback.reasoning) {
            if target.supported != Some(false) {
                if target.supported.is_none() {
                    target.supported = source.supported;
                }
                if target.supported_efforts.is_none() && source.supported_efforts.is_some() {
                    target.supported_efforts = source.supported_efforts.clone();
                    self.field_sources
                        .insert("reasoning.supported_efforts".into(), "models_dev".into());
                }
                if target.controls.is_empty() && !source.controls.is_empty() {
                    target.controls = source.controls.clone();
                    self.field_sources
                        .insert("reasoning.controls".into(), "models_dev".into());
                }
                for (name, target, source) in [
                    (
                        "reasoning.can_disable",
                        &mut target.can_disable,
                        source.can_disable,
                    ),
                    (
                        "reasoning.supports_max_tokens",
                        &mut target.supports_max_tokens,
                        source.supports_max_tokens,
                    ),
                    (
                        "reasoning.supports_dynamic_budget",
                        &mut target.supports_dynamic_budget,
                        source.supports_dynamic_budget,
                    ),
                ] {
                    if target.is_none()
                        && source.is_some()
                        && self
                            .field_sources
                            .get(name)
                            .is_none_or(|source| source != "provider_api")
                    {
                        *target = source;
                        self.field_sources.insert(name.into(), "models_dev".into());
                    }
                }
            }
        }
        self.updated_at = fallback.updated_at;
        self.stale = fallback.stale;
    }

    pub fn from_provider(entry: &Value) -> Self {
        let parameters = entry.get("supported_parameters").and_then(Value::as_array);
        let mut details = Self {
            input_modalities: modalities(entry.pointer("/architecture/input_modalities")),
            output_modalities: modalities(entry.pointer("/architecture/output_modalities")),
            context_tokens: positive_limit(entry.get("context_length")),
            max_output_tokens: positive_limit(entry.pointer("/top_provider/max_completion_tokens")),
            temperature: parameters.map(|values| values.iter().any(|value| value == "temperature")),
            tool_call: parameters.map(|values| values.iter().any(|value| value == "tools")),
            ..Default::default()
        };
        for (field, known) in [
            ("input_modalities", !details.input_modalities.is_empty()),
            ("output_modalities", !details.output_modalities.is_empty()),
            ("context_tokens", details.context_tokens.is_some()),
            ("max_output_tokens", details.max_output_tokens.is_some()),
            ("temperature", details.temperature.is_some()),
            ("tool_call", details.tool_call.is_some()),
        ] {
            if known {
                details
                    .field_sources
                    .insert(field.into(), "provider_api".into());
            }
        }
        details
    }

    pub fn from_native(entry: &Value, provider: &str) -> Self {
        let mut details = Self::default();
        match provider {
            "mimo" => {
                // /models returns IDs only. Capabilities come from the native model guide.
                // https://mimo.mi.com/docs/zh-CN/quick-start/summary/model
                let model = entry.get("id").and_then(Value::as_str).unwrap_or_default();
                if crate::providers::MIMO_CHAT_MODELS.contains(&model) {
                    details.input_modalities = vec!["text".into(), "image".into()];
                    details.output_modalities = vec!["text".into()];
                    details.context_tokens = Some(1_048_576);
                    details.max_output_tokens = Some(131_072);
                } else if model == "mimo-v2.5-asr" {
                    details.input_modalities = vec!["audio".into()];
                    details.output_modalities = vec!["text".into()];
                }
            }
            "gemini" => {
                details.context_tokens = positive_limit(entry.get("inputTokenLimit"));
                details.max_output_tokens = positive_limit(entry.get("outputTokenLimit"));
            }
            "groq" => {
                details.context_tokens = positive_limit(entry.get("context_window"));
                details.max_output_tokens = positive_limit(entry.get("max_completion_tokens"));
            }
            "deepseek" => {
                details.context_tokens = positive_limit(entry.get("context_window"));
                details.max_output_tokens = positive_limit(entry.get("max_output_tokens"));
                details.input_modalities = modalities(entry.get("input_modalities"));
                details.output_modalities = modalities(entry.get("output_modalities"));
                let efforts = modalities(entry.pointer("/effort/supported_levels"));
                if !efforts.is_empty() {
                    let default = entry
                        .pointer("/effort/default_level")
                        .and_then(Value::as_str)
                        .filter(|level| efforts.iter().any(|effort| effort == level))
                        .map(str::to_owned);
                    details.reasoning = Some(ReasoningOptions {
                        supported: Some(true),
                        mandatory: Some(false),
                        default_effort: default,
                        controls: vec![
                            ReasoningControl::Toggle,
                            ReasoningControl::Effort {
                                values: efforts.clone(),
                            },
                        ],
                        supported_efforts: Some(efforts),
                        can_disable: Some(true),
                        ..Default::default()
                    });
                }
            }
            "openai"
                if entry
                    .get("shutdown_date")
                    .and_then(Value::as_str)
                    .is_some_and(|date| !date.is_empty()) =>
            {
                details.status = Some("deprecated".into())
            }
            _ => {}
        }
        for (field, known) in [
            ("context_tokens", details.context_tokens.is_some()),
            ("max_output_tokens", details.max_output_tokens.is_some()),
            ("input_modalities", !details.input_modalities.is_empty()),
            ("output_modalities", !details.output_modalities.is_empty()),
            ("reasoning", details.reasoning.is_some()),
            ("status", details.status.is_some()),
        ] {
            if known {
                details.field_sources.insert(
                    field.into(),
                    if provider == "mimo" {
                        "builtin"
                    } else {
                        "provider_api"
                    }
                    .into(),
                );
            }
        }
        details
    }

    /// Apply public metadata only to protocols implemented by VRCS.
    pub fn apply_request(&self, body: &mut Value, provider: &str, enabled: bool) {
        let output_key = match provider {
            "gemini" => "/generationConfig/maxOutputTokens",
            "openai" => "/max_output_tokens",
            "mimo" => "/max_completion_tokens",
            _ => "/max_tokens",
        };
        if !self.stale
            || self
                .field_sources
                .get("max_output_tokens")
                .is_some_and(|s| s == "provider_api")
        {
            if let (Some(limit), Some(value)) =
                (self.max_output_tokens, body.pointer_mut(output_key))
            {
                if let Some(current) = value.as_u64() {
                    *value = json!(current.min(limit as u64));
                }
            }
        }
        if self.temperature == Some(false)
            && (!self.stale
                || self
                    .field_sources
                    .get("temperature")
                    .is_some_and(|s| s == "provider_api"))
        {
            let target = if provider == "gemini" {
                &mut body["generationConfig"]
            } else {
                &mut *body
            };
            if let Some(fields) = target.as_object_mut() {
                fields.remove("temperature");
            }
        }
        if self.stale
            && self
                .field_sources
                .get("reasoning")
                .is_none_or(|s| s != "provider_api")
        {
            return;
        }
        let Some(options) = self.reasoning.as_ref() else {
            return;
        };
        if options.supported == Some(false) {
            match provider {
                "openai" => {
                    if let Some(fields) = body.as_object_mut() {
                        fields.remove("reasoning");
                    }
                }
                "gemini" => {
                    if let Some(fields) = body["generationConfig"].as_object_mut() {
                        fields.remove("thinkingConfig");
                    }
                }
                "deepseek" | "groq" | "alibaba_cloud" | "alibaba_token_plan" => {
                    if let Some(fields) = body.as_object_mut() {
                        for field in [
                            "thinking",
                            "enable_thinking",
                            "reasoning_effort",
                            "include_reasoning",
                            "reasoning_format",
                        ] {
                            fields.remove(field);
                        }
                    }
                }
                _ => {}
            }
            return;
        }
        let has_toggle = (!self.stale
            || self
                .field_sources
                .get("reasoning.controls")
                .is_none_or(|s| s != "models_dev"))
            && options
                .controls
                .iter()
                .any(|control| matches!(control, ReasoningControl::Toggle));
        let efforts = options.supported_efforts.as_deref().filter(|_| {
            !self.stale
                || self
                    .field_sources
                    .get("reasoning.supported_efforts")
                    .is_none_or(|s| s != "models_dev")
        });
        let effort = efforts.and_then(|values| {
            let order: &[&str] = if enabled {
                &[
                    "medium", "high", "low", "minimal", "xhigh", "max", "default",
                ]
            } else if options.mandatory == Some(true) {
                &["minimal", "low", "medium", "high", "xhigh", "max"]
            } else {
                &["none", "minimal", "low", "medium", "high", "xhigh", "max"]
            };
            order
                .iter()
                .find(|candidate| {
                    let mapped = match provider {
                        "gemini" => ["minimal", "low", "medium", "high"].contains(candidate),
                        "groq" => ["none", "default", "low", "medium", "high"].contains(candidate),
                        _ => true,
                    };
                    mapped && values.iter().any(|value| value == **candidate)
                })
                .copied()
        });
        match provider {
            "openai" if effort.is_some() => {
                body["reasoning"] = json!({"effort": effort});
            }
            "gemini" if effort.is_some() => {
                body["generationConfig"]["thinkingConfig"] = json!({"thinkingLevel": effort});
            }
            "alibaba_cloud" | "alibaba_token_plan" if options.mandatory == Some(true) => {
                if let Some(fields) = body.as_object_mut() {
                    fields.remove("enable_thinking");
                }
            }
            "deepseek" if options.mandatory == Some(true) => {
                body["thinking"] = json!({"type": "enabled"});
                if effort.is_some() {
                    body["reasoning_effort"] = json!(effort);
                }
            }
            "alibaba_cloud" | "alibaba_token_plan"
                if has_toggle && options.mandatory != Some(true) =>
            {
                body["enable_thinking"] = json!(enabled);
            }
            "deepseek" if has_toggle && options.mandatory != Some(true) => {
                body["thinking"] = json!({"type": if enabled {"enabled"} else {"disabled"}});
                if enabled && effort.is_some() {
                    body["reasoning_effort"] = json!(effort);
                }
            }
            "groq" if effort.is_some() => {
                body["reasoning_effort"] = json!(effort);
            }
            _ => {}
        }
    }
}

pub(super) fn modalities(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .filter(|values| values.iter().all(Value::is_string))
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

pub(super) fn positive_limit(value: Option<&Value>) -> Option<u32> {
    value
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_provider_fields_override_public_facts_without_cross_provider_inference() {
        let entry = json!({"context_window": 10000, "max_output_tokens": 2000,
            "input_modalities": ["text", "image"], "output_modalities": ["text"],
            "effort": {"supported_levels": ["low", "high", "max"], "default_level": "high"}});
        let mut deepseek = ModelDetails::from_native(&entry, "deepseek");
        deepseek.supplement(&ModelDetails {
            max_output_tokens: Some(4000),
            input_modalities: vec!["text".into()],
            ..Default::default()
        });
        assert_eq!(deepseek.max_output_tokens, Some(2000));
        assert_eq!(deepseek.input_modalities, ["text", "image"]);
        assert_eq!(
            deepseek
                .reasoning
                .as_ref()
                .unwrap()
                .default_effort
                .as_deref(),
            Some("high")
        );
        for enabled in [true, false] {
            let mut body = json!({"max_tokens": 3000});
            deepseek.apply_request(&mut body, "deepseek", enabled);
            assert_eq!(body["max_tokens"], 2000);
            assert_eq!(
                body["thinking"]["type"],
                if enabled { "enabled" } else { "disabled" }
            );
            assert_eq!(
                body.get("reasoning_effort"),
                enabled.then_some(&json!("high"))
            );
        }
        let groq = ModelDetails::from_native(
            &json!({"context_window": 32000, "max_completion_tokens": 1024}),
            "groq",
        );
        assert_eq!(groq.context_tokens, Some(32000));
        assert_eq!(groq.max_output_tokens, Some(1024));
        assert!(groq.reasoning.is_none());
        let gemini = ModelDetails::from_native(
            &json!({"inputTokenLimit": 8000, "outputTokenLimit": 256}),
            "gemini",
        );
        assert_eq!(gemini.max_output_tokens, Some(256));
        assert_eq!(
            ModelDetails::from_native(&json!({"shutdown_date": "2026-10-23"}), "openai")
                .status
                .as_deref(),
            Some("deprecated")
        );
        for provider in [
            "openai",
            "alibaba_cloud",
            "alibaba_token_plan",
            "openai_compatible",
        ] {
            let unknown = ModelDetails::from_native(&entry, provider);
            assert!(unknown.max_output_tokens.is_none());
            assert!(unknown.input_modalities.is_empty());
            assert!(unknown.reasoning.is_none());
        }
    }

    #[test]
    fn reasoning_parameters_use_the_provider_protocol_and_unknown_controls_do_nothing() {
        let details = ModelDetails {
            reasoning: Some(ReasoningOptions {
                supported: Some(true),
                controls: vec![ReasoningControl::Toggle],
                supported_efforts: Some(vec![
                    "high".into(),
                    "none".into(),
                    "medium".into(),
                    "low".into(),
                ]),
                ..Default::default()
            }),
            ..Default::default()
        };
        for enabled in [true, false] {
            for (provider, field, expected) in [
                (
                    "openai",
                    "/reasoning/effort",
                    json!(if enabled { "medium" } else { "none" }),
                ),
                (
                    "gemini",
                    "/generationConfig/thinkingConfig/thinkingLevel",
                    json!(if enabled { "medium" } else { "low" }),
                ),
                (
                    "groq",
                    "/reasoning_effort",
                    json!(if enabled { "medium" } else { "none" }),
                ),
                (
                    "deepseek",
                    "/thinking/type",
                    json!(if enabled { "enabled" } else { "disabled" }),
                ),
                ("alibaba_cloud", "/enable_thinking", json!(enabled)),
                ("alibaba_token_plan", "/enable_thinking", json!(enabled)),
            ] {
                let mut body = json!({});
                details.apply_request(&mut body, provider, enabled);
                assert_eq!(body.pointer(field), Some(&expected), "{provider}");
            }
        }
        let mut body = json!({"model": "custom"});
        details.apply_request(&mut body, "openai_compatible", true);
        assert_eq!(body, json!({"model": "custom"}));
        let unknown = ModelDetails {
            reasoning: Some(ReasoningOptions {
                supported: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        unknown.apply_request(&mut body, "openai", false);
        assert_eq!(body, json!({"model": "custom"}));
    }

    #[test]
    fn limits_never_expand_a_callers_output_budget() {
        let details = ModelDetails {
            max_output_tokens: Some(128),
            temperature: Some(false),
            ..Default::default()
        };
        for (provider, mut body, pointer) in [
            (
                "openai",
                json!({"max_output_tokens": 256}),
                "/max_output_tokens",
            ),
            (
                "gemini",
                json!({"generationConfig": {"maxOutputTokens": 256, "temperature": 0}}),
                "/generationConfig/maxOutputTokens",
            ),
            (
                "groq",
                json!({"max_tokens": 256, "temperature": 0}),
                "/max_tokens",
            ),
        ] {
            details.apply_request(&mut body, provider, false);
            assert_eq!(body.pointer(pointer), Some(&json!(128)));
            assert!(body.get("temperature").is_none());
            assert!(body.pointer("/generationConfig/temperature").is_none());
        }
        let mut body = json!({"max_tokens": 32});
        details.apply_request(&mut body, "groq", false);
        assert_eq!(body["max_tokens"], 32);
    }

    #[test]
    fn required_reasoning_uses_semantic_effort_order_and_never_disables() {
        let details = ModelDetails {
            reasoning: Some(ReasoningOptions {
                supported: Some(true),
                mandatory: Some(true),
                supported_efforts: Some(vec![
                    "low".into(),
                    "none".into(),
                    "high".into(),
                    "minimal".into(),
                ]),
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut body = json!({"max_output_tokens": 128});
        details.apply_request(&mut body, "openai", false);
        assert_eq!(body["reasoning"], json!({"effort": "minimal"}));
        for provider in ["alibaba_cloud", "alibaba_token_plan"] {
            let mut body = json!({"enable_thinking": false});
            details.apply_request(&mut body, provider, false);
            assert!(body.get("enable_thinking").is_none());
        }
        let mut body = json!({"thinking": {"type": "disabled"}});
        details.apply_request(&mut body, "deepseek", false);
        assert_eq!(body["thinking"]["type"], "enabled");
    }

    #[test]
    fn stale_public_metadata_does_not_change_parameters_or_limits() {
        let details = ModelDetails {
            stale: true,
            max_output_tokens: Some(128),
            temperature: Some(false),
            reasoning: Some(ReasoningOptions {
                supported: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut body =
            json!({"max_tokens": 256, "temperature": 0, "thinking": {"type": "enabled"}});
        let original = body.clone();
        details.apply_request(&mut body, "deepseek", false);
        assert_eq!(body, original);
    }

    #[test]
    fn explicit_non_reasoning_provider_fact_does_not_inherit_controls() {
        let mut direct = ModelDetails {
            reasoning: Some(ReasoningOptions {
                supported: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        };
        let fallback = ModelDetails {
            reasoning: Some(ReasoningOptions {
                supported: Some(true),
                controls: vec![ReasoningControl::Toggle],
                ..Default::default()
            }),
            ..Default::default()
        };
        direct.supplement(&fallback);
        assert_eq!(direct.reasoning.as_ref().unwrap().supported, Some(false));
        assert!(direct.reasoning.as_ref().unwrap().controls.is_empty());
        let mut body = json!({"enable_thinking": true});
        direct.apply_request(&mut body, "alibaba_cloud", true);
        assert!(body.get("enable_thinking").is_none());
    }
}
