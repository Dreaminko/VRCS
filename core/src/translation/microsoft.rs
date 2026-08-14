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
    let mut request = http
        .post("https://api.cognitive.microsofttranslator.com/translate")
        .query(&[("api-version", "3.0"), ("to", language_code(target))])
        .header("Ocp-Apim-Subscription-Key", api_key)
        .header(
            "Ocp-Apim-Subscription-Region",
            profile.region.as_deref().unwrap_or(""),
        );
    if let Some(source) = source.filter(|value| *value != "auto") {
        request = request.query(&[("from", language_code(source))]);
    }
    let response = request
        .json(&json!([{ "Text": text }]))
        .send()
        .await
        .map_err(network_error)?;
    let status = response.status();
    let value: Value = response.json().await.map_err(invalid_response)?;
    if !status.is_success() {
        return Err(http_error(status, &value));
    }
    let translated = value
        .pointer("/0/translations/0/text")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid("Microsoft Translator response did not contain translated text"))?;
    let detected = value
        .pointer("/0/detectedLanguage/language")
        .and_then(Value::as_str)
        .map(str::to_owned);
    Ok(TranslationResult {
        text: translated.to_owned(),
        source_language: source.map(str::to_owned).or(detected),
        target_language: target.to_owned(),
        provider: profile.provider.clone(),
        model: None,
    })
}

fn language_code(language: &str) -> &'static str {
    match language {
        "zh-Hans" | "zh" => "zh-Hans",
        "zh-Hant" => "zh-Hant",
        "en" => "en",
        "ja" => "ja",
        "ko" => "ko",
        "es" => "es",
        "fr" => "fr",
        "de" => "de",
        "ru" => "ru",
        _ => "en",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_supported_language_codes() {
        assert_eq!(language_code("zh-Hant"), "zh-Hant");
        assert_eq!(language_code("zh"), "zh-Hans");
    }
}
