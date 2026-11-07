#[cfg(test)]
pub(super) use crate::chatbox::CHATBOX_LIMIT;

pub(super) fn format_chatbox(
    original: &str,
    translation: Option<&str>,
    preserve_original_text: bool,
) -> String {
    let compact_original = crate::chatbox::compact_text(original);
    let compact_translation = translation.map(crate::chatbox::compact_text);
    let translation = compact_translation
        .as_deref()
        .filter(|value| !value.is_empty() && *value != compact_original.as_str());
    crate::chatbox::automatic_chatbox(original, translation, preserve_original_text)
}
