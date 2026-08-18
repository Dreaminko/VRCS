use serde_json::{json, Value};

use crate::providers::OpenAiProtocolBehavior;

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
            "qwen/qwen3.6-27b" | "minimaxai/minimax-m2.7"
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
        "minimaxai/minimax-m2.7" => {
            body["reasoning_format"] = json!("hidden");
        }
        _ => {}
    }
}

fn groq_thinking_control(model: &str) -> ThinkingControl {
    match normalized_model(model) {
        "qwen/qwen3.6-27b" => ThinkingControl::DisableSupported,
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
    model == "qwq-plus"
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
    model.starts_with("qwen3.7-max")
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
            thinking_control(OpenAiProtocolBehavior::Alibaba, "deepseek-r1"),
            ThinkingControl::HideOnly
        );
        assert_eq!(
            thinking_control(OpenAiProtocolBehavior::Standard, "qwen/qwen3.6-27b"),
            ThinkingControl::Unsupported
        );
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
