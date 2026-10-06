pub fn parse_shortcut(value: &str) -> Result<(u32, u32), String> {
    let invalid = || "Invalid OCR shortcut".to_string();
    let parts: Vec<_> = value.split('+').map(str::trim).collect();
    let (key, modifiers) = parts.split_last().ok_or_else(invalid)?;
    let mut flags = 0;
    for modifier in modifiers {
        let flag = match modifier.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => 2,
            "alt" => 1,
            "shift" => 4,
            _ => return Err(invalid()),
        };
        if flags & flag != 0 {
            return Err(invalid());
        }
        flags |= flag;
    }
    if flags == 0 {
        return Err(invalid());
    }
    let key = key.to_ascii_uppercase();
    let vk = if key.len() == 1 && key.as_bytes()[0].is_ascii_alphanumeric() {
        key.as_bytes()[0] as u32
    } else if let Some(number) = key
        .strip_prefix('F')
        .and_then(|value| value.parse::<u32>().ok())
    {
        if !(1..=11).contains(&number) {
            return Err(invalid());
        }
        111 + number
    } else {
        return Err(invalid());
    };
    Ok((flags, vk))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modifiers_letters_numbers_and_function_keys() {
        assert_eq!(parse_shortcut("Ctrl+Alt+O").unwrap(), (3, 79));
        assert_eq!(parse_shortcut(" shift + ctrl + f8 ").unwrap(), (6, 119));
        assert_eq!(parse_shortcut("Alt+5").unwrap(), (1, 53));
    }

    #[test]
    fn rejects_incomplete_reserved_or_ambiguous_shortcuts() {
        for value in [
            "",
            "O",
            "Ctrl",
            "Ctrl+Alt",
            "Ctrl+O+P",
            "Ctrl+Ctrl+O",
            "Ctrl+F12",
            "Win+O",
            "Ctrl+Space",
        ] {
            assert!(parse_shortcut(value).is_err(), "{value}");
        }
    }
}
