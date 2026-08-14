use serde::{Deserialize, Serialize};

pub const CHATBOX_LIMIT: usize = 144;
const MAX_TEXT_LENGTH: usize = 5_000;
const MAX_CUSTOM_FORMAT_LENGTH: usize = 200;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChatboxSendMode {
    Original,
    Translation,
    Bilingual,
}

impl ChatboxSendMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Original => "original",
            Self::Translation => "translation",
            Self::Bilingual => "bilingual",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChatboxMessageFormat {
    OriginalNewlineTranslation,
    TranslationNewlineOriginal,
    SlashSeparated,
    Custom,
}

impl ChatboxMessageFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OriginalNewlineTranslation => "original_newline_translation",
            Self::TranslationNewlineOriginal => "translation_newline_original",
            Self::SlashSeparated => "slash_separated",
            Self::Custom => "custom",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChatboxOverflowPolicy {
    Block,
    SmartTruncate,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChatboxComposeInput {
    pub original: String,
    #[serde(default)]
    pub translation: Option<String>,
    #[serde(default)]
    pub source_language: Option<String>,
    #[serde(default)]
    pub target_language: Option<String>,
    pub send_mode: ChatboxSendMode,
    pub message_format: ChatboxMessageFormat,
    #[serde(default)]
    pub custom_format: Option<String>,
    pub overflow_policy: ChatboxOverflowPolicy,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ChatboxPreview {
    pub text: String,
    pub char_count: usize,
    pub limit: usize,
    pub over_limit: bool,
    pub truncated: bool,
    pub sendable: bool,
}

#[derive(Debug, Clone)]
pub struct ChatboxValidationError {
    pub code: &'static str,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatboxMessage {
    pub id: i64,
    pub source: String,
    pub original: String,
    pub translation: Option<String>,
    pub source_language: Option<String>,
    pub target_language: Option<String>,
    pub send_mode: String,
    pub message_format: String,
    pub custom_format: Option<String>,
    pub rendered_text: String,
    pub char_count: usize,
    pub truncated: bool,
    pub status: String,
    pub error_code: Option<String>,
    pub error_detail: Option<String>,
    pub resent_from_id: Option<i64>,
    pub created_at: String,
    pub sent_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewChatboxMessage {
    pub source: String,
    pub original: String,
    pub translation: Option<String>,
    pub source_language: Option<String>,
    pub target_language: Option<String>,
    pub send_mode: String,
    pub message_format: String,
    pub custom_format: Option<String>,
    pub rendered_text: String,
    pub char_count: usize,
    pub truncated: bool,
    pub status: String,
    pub error_code: Option<String>,
    pub error_detail: Option<String>,
    pub resent_from_id: Option<i64>,
    pub created_at: String,
    pub sent_at: Option<String>,
}

pub fn preview_chatbox(
    input: &ChatboxComposeInput,
) -> Result<ChatboxPreview, ChatboxValidationError> {
    validate_input(input)?;
    let original = compact_text(&input.original);
    let translation = input.translation.as_deref().map(compact_text);
    let template = format_template(input)?;
    let full_text = render(&template, &original, translation.as_deref());
    let full_length = full_text.chars().count();

    if full_length <= CHATBOX_LIMIT {
        return Ok(ChatboxPreview {
            text: full_text,
            char_count: full_length,
            limit: CHATBOX_LIMIT,
            over_limit: false,
            truncated: false,
            sendable: true,
        });
    }

    if input.overflow_policy == ChatboxOverflowPolicy::Block {
        return Ok(ChatboxPreview {
            text: full_text,
            char_count: full_length,
            limit: CHATBOX_LIMIT,
            over_limit: true,
            truncated: false,
            sendable: false,
        });
    }

    let text = match input.send_mode {
        ChatboxSendMode::Original => truncate(&original, CHATBOX_LIMIT),
        ChatboxSendMode::Translation => {
            truncate(translation.as_deref().unwrap_or_default(), CHATBOX_LIMIT)
        }
        ChatboxSendMode::Bilingual => truncate_bilingual(
            &template,
            &original,
            translation.as_deref().unwrap_or_default(),
        )?,
    };
    Ok(ChatboxPreview {
        char_count: text.chars().count(),
        text,
        limit: CHATBOX_LIMIT,
        over_limit: true,
        truncated: true,
        sendable: true,
    })
}

pub fn automatic_chatbox(original: &str, translation: Option<&str>) -> String {
    let input = ChatboxComposeInput {
        original: original.into(),
        translation: translation.map(str::to_owned),
        source_language: None,
        target_language: None,
        send_mode: if translation.is_some() {
            ChatboxSendMode::Bilingual
        } else {
            ChatboxSendMode::Original
        },
        message_format: ChatboxMessageFormat::OriginalNewlineTranslation,
        custom_format: None,
        overflow_policy: ChatboxOverflowPolicy::SmartTruncate,
    };
    preview_chatbox(&input)
        .map(|preview| preview.text)
        .unwrap_or_default()
}

fn validate_input(input: &ChatboxComposeInput) -> Result<(), ChatboxValidationError> {
    validate_optional_language(input.source_language.as_deref())?;
    validate_optional_language(input.target_language.as_deref())?;
    validate_text("original", &input.original)?;
    if let Some(translation) = input.translation.as_deref() {
        validate_text("translation", translation)?;
    }

    let original = compact_text(&input.original);
    let translation = input.translation.as_deref().map(compact_text);
    match input.send_mode {
        ChatboxSendMode::Original if original.is_empty() => Err(validation(
            "chatbox.missing_original",
            "Original text is required",
        )),
        ChatboxSendMode::Translation if translation.as_deref().unwrap_or_default().is_empty() => {
            Err(validation(
                "chatbox.missing_translation",
                "Translation text is required",
            ))
        }
        ChatboxSendMode::Bilingual
            if original.is_empty() || translation.as_deref().unwrap_or_default().is_empty() =>
        {
            Err(validation(
                "chatbox.missing_bilingual_text",
                "Original and translation text are required",
            ))
        }
        _ => Ok(()),
    }
}

fn validate_text(label: &str, value: &str) -> Result<(), ChatboxValidationError> {
    if value.chars().count() > MAX_TEXT_LENGTH {
        return Err(validation(
            "chatbox.text_too_long",
            format!("{label} cannot exceed {MAX_TEXT_LENGTH} characters"),
        ));
    }
    Ok(())
}

fn validate_optional_language(value: Option<&str>) -> Result<(), ChatboxValidationError> {
    if value.is_some_and(|language| language.chars().count() > 20) {
        return Err(validation(
            "chatbox.invalid_language",
            "Language code cannot exceed 20 characters",
        ));
    }
    Ok(())
}

fn format_template(input: &ChatboxComposeInput) -> Result<String, ChatboxValidationError> {
    match input.send_mode {
        ChatboxSendMode::Original => Ok("{original}".into()),
        ChatboxSendMode::Translation => Ok("{translation}".into()),
        ChatboxSendMode::Bilingual => match input.message_format {
            ChatboxMessageFormat::OriginalNewlineTranslation => {
                Ok("{original}\n{translation}".into())
            }
            ChatboxMessageFormat::TranslationNewlineOriginal => {
                Ok("{translation}\n{original}".into())
            }
            ChatboxMessageFormat::SlashSeparated => Ok("{original} / {translation}".into()),
            ChatboxMessageFormat::Custom => {
                validate_custom_format(input.custom_format.as_deref().unwrap_or_default())
            }
        },
    }
}

fn validate_custom_format(value: &str) -> Result<String, ChatboxValidationError> {
    let length = value.chars().count();
    let original_count = value.matches("{original}").count();
    let translation_count = value.matches("{translation}").count();
    if length == 0
        || length > MAX_CUSTOM_FORMAT_LENGTH
        || original_count != 1
        || translation_count != 1
    {
        return Err(validation(
            "chatbox.invalid_format",
            "Custom format must contain {original} and {translation} exactly once",
        ));
    }
    let remainder = value.replace("{original}", "").replace("{translation}", "");
    if remainder.contains('{') || remainder.contains('}') || remainder.chars().any(char::is_control)
    {
        return Err(validation(
            "chatbox.invalid_format",
            "Custom format contains unsupported placeholders or control characters",
        ));
    }
    Ok(value.into())
}

fn truncate_bilingual(
    template: &str,
    original: &str,
    translation: &str,
) -> Result<String, ChatboxValidationError> {
    let fixed = template
        .replace("{original}", "")
        .replace("{translation}", "")
        .chars()
        .count();
    if fixed >= CHATBOX_LIMIT {
        return Err(validation(
            "chatbox.invalid_format",
            "Message format leaves no room for text",
        ));
    }
    let available = CHATBOX_LIMIT - fixed;
    let original_length = original.chars().count();
    let translation_length = translation.chars().count();
    let mut original_budget = available / 2;
    let mut translation_budget = available - original_budget;
    if original_length < original_budget {
        translation_budget += original_budget - original_length;
        original_budget = original_length;
    } else if translation_length < translation_budget {
        original_budget += translation_budget - translation_length;
        translation_budget = translation_length;
    }
    Ok(render(
        template,
        &truncate(original, original_budget),
        Some(&truncate(translation, translation_budget)),
    ))
}

fn render(template: &str, original: &str, translation: Option<&str>) -> String {
    template
        .replace("{original}", original)
        .replace("{translation}", translation.unwrap_or_default())
}

pub(crate) fn compact_text(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn truncate(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value.into();
    }
    if limit == 0 {
        return String::new();
    }
    value
        .chars()
        .take(limit - 1)
        .chain(std::iter::once('…'))
        .collect()
}

fn validation(code: &'static str, detail: impl Into<String>) -> ChatboxValidationError {
    ChatboxValidationError {
        code,
        detail: detail.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(original: &str, translation: Option<&str>) -> ChatboxComposeInput {
        ChatboxComposeInput {
            original: original.into(),
            translation: translation.map(str::to_owned),
            source_language: None,
            target_language: Some("ja".into()),
            send_mode: if translation.is_some() {
                ChatboxSendMode::Bilingual
            } else {
                ChatboxSendMode::Original
            },
            message_format: ChatboxMessageFormat::OriginalNewlineTranslation,
            custom_format: None,
            overflow_policy: ChatboxOverflowPolicy::Block,
        }
    }

    #[test]
    fn previews_and_blocks_oversized_messages() {
        let preview = preview_chatbox(&input(&"原".repeat(145), None)).unwrap();
        assert_eq!(preview.char_count, 145);
        assert!(preview.over_limit);
        assert!(!preview.sendable);
    }

    #[test]
    fn smart_truncation_preserves_both_languages() {
        let mut input = input(&"原".repeat(100), Some(&"訳".repeat(100)));
        input.overflow_policy = ChatboxOverflowPolicy::SmartTruncate;
        let preview = preview_chatbox(&input).unwrap();
        let lines = preview.text.lines().collect::<Vec<_>>();
        assert_eq!(preview.char_count, CHATBOX_LIMIT);
        assert_eq!(lines[0].chars().count(), 71);
        assert_eq!(lines[1].chars().count(), 72);
        assert!(preview.truncated);
    }

    #[test]
    fn validates_custom_placeholders() {
        let mut input = input("hello", Some("こんにちは"));
        input.message_format = ChatboxMessageFormat::Custom;
        input.custom_format = Some("{original} → {translation}".into());
        assert_eq!(preview_chatbox(&input).unwrap().text, "hello → こんにちは");

        input.custom_format = Some("{original}".into());
        assert_eq!(
            preview_chatbox(&input).unwrap_err().code,
            "chatbox.invalid_format"
        );
    }
}
