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
    let target_code = language_code(target).ok_or_else(|| {
        invalid(format!(
            "Microsoft Translator does not support target language: {target}"
        ))
    })?;
    let mut request = http
        .post("https://api.cognitive.microsofttranslator.com/translate")
        .query(&[("api-version", "3.0"), ("to", target_code)])
        .header("Ocp-Apim-Subscription-Key", api_key)
        .header(
            "Ocp-Apim-Subscription-Region",
            profile.region.as_deref().unwrap_or(""),
        );
    if let Some(source) = source.filter(|value| *value != "auto") {
        if let Some(source_code) = language_code(source) {
            request = request.query(&[("from", source_code)]);
        }
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

fn language_code(language: &str) -> Option<&str> {
    match language {
        "zh" => Some("zh-Hans"),
        value if crate::providers::LLM_TRANSLATION_LANGUAGES.contains(&value) => Some(value),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_supported_language_codes() {
        assert_eq!(language_code("zh-Hant"), Some("zh-Hant"));
        assert_eq!(language_code("zh"), Some("zh-Hans"));
        assert_eq!(language_code("tlh"), None);
    }
}
