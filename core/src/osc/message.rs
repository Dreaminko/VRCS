#[cfg(test)]
pub(super) use crate::chatbox::CHATBOX_LIMIT;

#[cfg(test)]
pub(super) fn format_chatbox(
    original: &str,
    translation: Option<&str>,
    preserve_original_text: bool,
) -> String {
    format_chatbox_preview(
        original,
        translation,
        preserve_original_text,
        crate::chatbox::ChatboxMessageFormat::OriginalNewlineTranslation,
        None,
    )
    .text
}

pub(super) fn format_chatbox_preview(
    original: &str,
    translation: Option<&str>,
    preserve_original_text: bool,
    message_format: crate::chatbox::ChatboxMessageFormat,
    custom_format: Option<&str>,
) -> crate::chatbox::ChatboxPreview {
    let compact_original = crate::chatbox::compact_text(original);
    let compact_translation = translation.map(crate::chatbox::compact_text);
    let translation = compact_translation
        .as_deref()
        .filter(|value| !value.is_empty() && *value != compact_original.as_str());
    crate::chatbox::automatic_chatbox_preview(
        original,
        translation,
        preserve_original_text,
        message_format,
        custom_format,
    )
}
