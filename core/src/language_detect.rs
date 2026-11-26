use serde_json::{Value, json};
use whatlang::{Lang, Script};

const MAX_SAMPLE_CHARS: usize = 512;

pub fn has_explicit_language(value: Option<&str>) -> bool {
    let Some(trimmed) = value.map(str::trim).filter(|s| !s.is_empty()) else {
        return false;
    };
    !matches!(
        trimmed.to_ascii_lowercase().as_str(),
        "auto" | "und" | "unknown" | "mul" | "zxx"
    )
}

fn is_kana(ch: char) -> bool {
    matches!(ch, '\u{3040}'..='\u{309F}' | '\u{30A0}'..='\u{30FF}' | '\u{FF66}'..='\u{FF9F}')
}

fn is_hangul(ch: char) -> bool {
    matches!(ch, '\u{AC00}'..='\u{D7AF}' | '\u{1100}'..='\u{11FF}' | '\u{3130}'..='\u{318F}')
}

fn is_han(ch: char) -> bool {
    matches!(ch, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}')
}

fn has_chinese_marker(s: &str) -> bool {
    s.chars().any(|ch| {
        matches!(
            ch,
            '你' | '们'
                | '們'
                | '这'
                | '這'
                | '谢'
                | '謝'
                | '没'
                | '沒'
                | '吗'
                | '嗎'
                | '呢'
                | '么'
                | '麼'
                | '为'
                | '為'
                | '请'
                | '請'
                | '对'
                | '對'
                | '谁'
                | '誰'
                | '哪'
                | '啥'
                | '咱'
                | '您'
                | '吧'
                | '个'
                | '個'
                | '测'
                | '測'
                | '试'
                | '試'
                | '检'
                | '檢'
                | '繁'
                | '简'
                | '簡'
                | '滤'
                | '濾'
        )
    })
}

fn lang_code(lang: Lang) -> &'static str {
    match lang {
        Lang::Eng => "en",
        Lang::Fra => "fr",
        Lang::Deu => "de",
        Lang::Spa => "es",
        Lang::Ita => "it",
        Lang::Por => "pt",
        Lang::Rus => "ru",
        Lang::Nld => "nl",
        Lang::Pol => "pl",
        Lang::Tur => "tr",
        Lang::Vie => "vi",
        Lang::Tha => "th",
        Lang::Ara => "ar",
        Lang::Ind => "id",
        Lang::Swe => "sv",
        Lang::Ukr => "uk",
        Lang::Ces => "cs",
        Lang::Ron => "ro",
        Lang::Hun => "hu",
        Lang::Fin => "fi",
        Lang::Dan => "da",
        Lang::Nob => "nb",
        Lang::Ell => "el",
        Lang::Heb => "he",
        Lang::Hin => "hi",
        Lang::Cmn => "zh",
        Lang::Jpn => "ja",
        Lang::Kor => "ko",
        other => other.code(),
    }
}

pub fn detect_language(text: &str) -> Option<&'static str> {
    let sampled: String = text.trim().chars().take(MAX_SAMPLE_CHARS).collect();
    if !sampled.chars().any(char::is_alphabetic) {
        return None;
    }

    let mut has_kana = false;
    let mut has_hangul = false;
    let mut han_count = 0usize;
    let mut alpha_count = 0usize;

    for ch in sampled.chars() {
        if is_kana(ch) {
            has_kana = true;
        } else if is_hangul(ch) {
            has_hangul = true;
        } else if is_han(ch) {
            han_count += 1;
        }
        if ch.is_alphabetic() {
            alpha_count += 1;
        }
    }

    let zh_marker = has_chinese_marker(&sampled);
    if has_kana {
        return if has_hangul || zh_marker {
            None
        } else {
            Some("ja")
        };
    }
    if has_hangul {
        return if zh_marker { None } else { Some("ko") };
    }
    if han_count > 0 {
        if zh_marker {
            return Some("zh");
        }
        if han_count >= 5
            && let Some(info) = whatlang::detect(&sampled)
            && info.lang() == Lang::Cmn
        {
            return Some("zh");
        }
        return None;
    }

    let info = whatlang::detect(&sampled)?;
    if info.confidence() < 0.08 {
        return None;
    }
    if info.script() == Script::Latin && sampled.split_whitespace().count() < 2 && alpha_count < 10
    {
        return None;
    }
    Some(lang_code(info.lang()))
}

pub fn attach_detected_language_json(obj: &mut Value, text: &str) {
    let Some(map) = obj.as_object_mut() else {
        return;
    };
    let explicit = map.get("language").and_then(Value::as_str);
    if !has_explicit_language(explicit)
        && let Some(lang) = detect_language(text)
    {
        map.insert("detected_language".into(), json!(lang));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_cjk_latin_and_skips_ambiguous_inputs() {
        for text in [
            "这是一段用于测试的中文",
            "今天我们准备检查中文过滤功能",
            "這是繁體中文的測試",
            "你好",
            "谢谢",
        ] {
            assert_eq!(detect_language(text), Some("zh"), "{text}");
        }
        for text in [
            "同時通訳ってすごいですね",
            "今日は友達と話しています",
            "大丈夫です",
        ] {
            assert_eq!(detect_language(text), Some("ja"), "{text}");
        }
        assert_eq!(
            detect_language("오늘 친구들과 이야기하고 있어요"),
            Some("ko")
        );
        assert_eq!(detect_language("Thank you very much"), Some("en"));
        assert_eq!(
            detect_language("Ceci est une phrase française pour tester la traduction."),
            Some("fr")
        );
        for text in [
            "東京",
            "漢字",
            "大丈夫",
            "okay",
            "… 123 🙂",
            "",
            "你好こんにちは",
        ] {
            assert_eq!(detect_language(text), None, "{text}");
        }
    }
}
