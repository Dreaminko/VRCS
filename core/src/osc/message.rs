pub(super) const CHATBOX_LIMIT: usize = 144;

pub(super) fn format_chatbox(original: &str, translation: Option<&str>) -> String {
    let original = compact_line(original);
    let translation = translation
        .map(compact_line)
        .filter(|value| value != &original);
    match translation {
        None => truncate(&original, CHATBOX_LIMIT),
        Some(translation) => {
            let combined = format!("{original}\n{translation}");
            if combined.chars().count() <= CHATBOX_LIMIT {
                combined
            } else {
                format!(
                    "{}\n{}",
                    truncate(&original, 71),
                    truncate(&translation, 72)
                )
            }
        }
    }
}

fn compact_line(value: &str) -> String {
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
        return value.to_owned();
    }
    value
        .chars()
        .take(limit.saturating_sub(1))
        .chain(std::iter::once('…'))
        .collect()
}
