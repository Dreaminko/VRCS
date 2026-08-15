use serde_json::{json, Value};

use crate::config::ApiProfile;

use super::{
    http_error, invalid, invalid_response, network_error, TranslationError, TranslationResult,
};

pub(super) async fn translate(
    http: &reqwest::Client,
    profile: &ApiProfile,
    api_key: &str,
    text: &str,
    source: Option<&str>,
    target: &str,
) -> Result<TranslationResult, TranslationError> {
    let target_code = language_code(target)
        .ok_or_else(|| invalid(format!("DeepL does not support target language: {target}")))?;
    let endpoint = if api_key.ends_with(":fx") {
        "https://api-free.deepl.com/v2/translate"
    } else {
        "https://api.deepl.com/v2/translate"
    };
    let mut body = json!({
        "text": [text],
        "target_lang": target_code,
        "preserve_formatting": true
    });
    if let Some(source) = source.filter(|value| *value != "auto") {
        if let Some(source_code) = language_code(source) {
            body["source_lang"] = json!(source_code);
        }
    }
    let response = http
        .post(endpoint)
        .header("Authorization", format!("DeepL-Auth-Key {api_key}"))
        .json(&body)
        .send()
        .await
        .map_err(network_error)?;
    let status = response.status();
    let value: Value = response.json().await.map_err(invalid_response)?;
    if !status.is_success() {
        return Err(http_error(status, &value));
    }
    let translated = value
        .pointer("/translations/0/text")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid("DeepL response did not contain translated text"))?;
    let detected = value
        .pointer("/translations/0/detected_source_language")
        .and_then(Value::as_str)
        .map(|value| value.to_ascii_lowercase());
    Ok(TranslationResult {
        text: translated.to_owned(),
        source_language: source.map(str::to_owned).or(detected),
        target_language: target.to_owned(),
        provider: profile.provider.clone(),
        model: None,
    })
}

fn language_code(language: &str) -> Option<&'static str> {
    Some(match language {
        "zh-Hans" | "zh" => "ZH-HANS",
        "zh-Hant" => "ZH-HANT",
        "en" => "EN",
        "ja" => "JA",
        "ko" => "KO",
        "es" => "ES",
        "fr" => "FR",
        "de" => "DE",
        "ru" => "RU",
        "ar" => "AR",
        "bg" => "BG",
        "cs" => "CS",
        "da" => "DA",
        "el" => "EL",
        "he" => "HE",
        "id" => "ID",
        "it" => "IT",
        "nb" => "NB",
        "nl" => "NL",
        "pl" => "PL",
        "pt-BR" => "PT-BR",
        "pt-PT" => "PT-PT",
        "ro" => "RO",
        "sv" => "SV",
        "th" => "TH",
        "tr" => "TR",
        "uk" => "UK",
        "vi" => "VI",
        "hu" => "HU",
        "fi" => "FI",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_supported_language_codes() {
        assert_eq!(language_code("zh-Hans"), Some("ZH-HANS"));
        assert_eq!(language_code("pt-BR"), Some("PT-BR"));
        assert_eq!(language_code("yue-Hant"), None);
    }
}
