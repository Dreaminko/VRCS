use crate::config::GlossaryEntry;
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::Value;

use super::{TranslationContextEntry, TranslationError};

pub(crate) const MODEL_ID: &str = "plamo-3-translate-0.8b-q4_k_m";
pub(crate) const MODEL_FILENAME: &str = "plamo-3-translate-0.8b-Q4_K_M.gguf";
pub(crate) const MODEL_REPOSITORY: &str = "pfnet/plamo-3-translate-0.8b-GGUF";
pub(crate) const MODEL_REVISION: &str = "dbeb0856cb0b004b484f190bb71dd65876297013";
pub(crate) const MODEL_BYTES: u64 = 573_123_200;
pub(crate) const MODEL_SHA256: &str =
    "923b2f28a9856de1b8b3b610a7e59ded0b14d007466fd5dd526e6a9ecaadafdd";
pub(crate) const PROMPT_VERSION: &str = "pfml3-v1";
const MAX_PROMPT_TOKENS: usize = 3_008;
const MAX_OUTPUT_TOKENS: u32 = 1_024;
const STOP_MARKERS: [&str; 2] = ["<|plamo:tag|>", "<|plamo:bos|>"];
pub(crate) const SUPPORTED_LANGUAGE_CODES: [&str; 53] = [
    "ja", "en", "zh-hans", "zh-hant", "ko", "ar", "az", "be", "bg", "bn", "ca", "ceb", "de", "es",
    "et", "eu", "fa", "fr", "gu", "he", "hi", "hr", "hu", "hy", "id", "it", "jv", "ka", "kk", "km",
    "lt", "lv", "ml", "mr", "ms", "my", "ne", "nl", "pa", "pl", "ru", "si", "sk", "sl", "sr", "sw",
    "ta", "te", "th", "tl", "ur", "uz", "vi",
];

pub(crate) fn supports_language(language: &str) -> bool {
    canonical_language(language)
        .is_some_and(|language| SUPPORTED_LANGUAGE_CODES.contains(&language.as_str()))
}

pub(crate) fn build_prompt(
    text: &str,
    source: Option<&str>,
    target: &str,
    context: &[TranslationContextEntry],
    glossary: &[GlossaryEntry],
) -> Result<String, TranslationError> {
    let target = canonical_language(target).ok_or_else(|| {
        super::error(
            "translation.invalid_target_language",
            format!("PLaMo does not support target language: {target}"),
            false,
        )
    })?;
    validate_content("translation text", text)?;

    let mut prompt = "<|plamo:tag|>dataset<|plamo:msg|>translation".to_owned();
    if !context.is_empty() {
        let body = context
            .iter()
            .map(|entry| entry.text.trim())
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if !body.is_empty() {
            validate_content("translation context", &body)?;
            prompt.push_str("<|plamo:tag|>context<|plamo:msg|>");
            prompt.push_str(&body);
        }
    }
    let glossary = glossary
        .iter()
        .filter_map(|entry| {
            let source = entry.source.trim();
            let target = entry.target.as_deref()?.trim();
            (!source.is_empty() && !target.is_empty()).then_some((source, target))
        })
        .collect::<Vec<_>>();
    if !glossary.is_empty() {
        for (source, target) in &glossary {
            validate_content("translation glossary", source)?;
            validate_content("translation glossary", target)?;
        }
        prompt.push_str("<|plamo:tag|>glossary");
        for (source, target) in glossary {
            prompt.push_str("<|plamo:index|>");
            prompt.push_str(source);
            prompt.push_str("<|plamo:item|>");
            prompt.push_str(target);
        }
    }
    prompt.push_str("<|plamo:tag|>input");
    if let Some(source) = source.and_then(canonical_language) {
        prompt.push_str("<|plamo:key|>lang<|plamo:val|>");
        prompt.push_str(&source);
    }
    prompt.push_str("<|plamo:msg|>");
    prompt.push_str(text.trim());
    prompt.push_str("<|plamo:tag|>output<|plamo:key|>lang<|plamo:val|>");
    prompt.push_str(&target);
    prompt.push_str("<|plamo:msg|>");
    Ok(prompt)
}

pub(crate) struct CompletionRequest<'a> {
    pub(crate) text: &'a str,
    pub(crate) source: Option<&'a str>,
    pub(crate) target: &'a str,
    pub(crate) context: &'a [TranslationContextEntry],
    pub(crate) glossary: &'a [GlossaryEntry],
    pub(crate) max_chars: usize,
}

pub(crate) async fn translate(
    http: &reqwest::Client,
    origin: &str,
    token: &str,
    request: CompletionRequest<'_>,
    on_progress: Option<&super::LlmProgress>,
) -> Result<String, TranslationError> {
    let prompt = fit_prompt(http, origin, token, &request).await?;
    let body = serde_json::json!({
        "prompt": prompt,
        "temperature": 0,
        "max_tokens": MAX_OUTPUT_TOKENS,
        "stream": true,
        "stop": STOP_MARKERS,
    });
    let response = http
        .post(format!("{origin}/v1/completions"))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .map_err(super::network_error)?;
    if !response.status().is_success() {
        let status = response.status();
        let value = response.json::<Value>().await.unwrap_or(Value::Null);
        return Err(super::http_error(status, &value));
    }
    let mut stream = response.bytes_stream();
    let mut buffer = Vec::new();
    let mut output = String::new();
    let mut finish_reason = None;
    let mut done = false;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| {
            super::error(
                "translation.local_stream_interrupted",
                format!(
                    "Local PLaMo stream was interrupted: {}",
                    error.without_url()
                ),
                true,
            )
        })?;
        buffer.extend_from_slice(&chunk);
        while let Some(newline) = buffer.iter().position(|byte| *byte == b'\n') {
            let line = buffer.drain(..=newline).collect::<Vec<_>>();
            process_line(
                &line,
                &mut output,
                &mut finish_reason,
                &mut done,
                on_progress,
            )?;
            if done {
                break;
            }
        }
        if done {
            break;
        }
    }
    if !done && !buffer.is_empty() {
        process_line(
            &buffer,
            &mut output,
            &mut finish_reason,
            &mut done,
            on_progress,
        )?;
    }
    if finish_reason.as_deref() == Some("length") {
        return Err(super::error(
            "translation.local_output_truncated",
            "Local PLaMo reached its output token limit",
            false,
        ));
    }
    if !done && finish_reason.as_deref() != Some("stop") {
        return Err(super::error(
            "translation.local_stream_interrupted",
            "Local PLaMo response ended before a completion marker",
            true,
        ));
    }
    let output = output.trim();
    if output.is_empty() {
        return Err(super::error(
            "translation.empty_result",
            "Local PLaMo returned an empty translation",
            false,
        ));
    }
    if output.contains("<|plamo:") {
        return Err(super::invalid(
            "Local PLaMo returned a reserved control marker",
        ));
    }
    Ok(output.to_owned())
}

async fn fit_prompt(
    http: &reqwest::Client,
    origin: &str,
    token: &str,
    request: &CompletionRequest<'_>,
) -> Result<String, TranslationError> {
    let context_start = context_start_within_budget(request.context, request.max_chars);
    for context_start in context_start..request.context.len() {
        let context = &request.context[context_start..];
        let prompt = build_prompt(
            request.text,
            request.source,
            request.target,
            context,
            request.glossary,
        )?;
        if token_count(http, origin, token, &prompt).await? <= MAX_PROMPT_TOKENS {
            return Ok(prompt);
        }
    }
    for glossary_len in (0..=request.glossary.len()).rev() {
        let prompt = build_prompt(
            request.text,
            request.source,
            request.target,
            &[],
            &request.glossary[..glossary_len],
        )?;
        if token_count(http, origin, token, &prompt).await? <= MAX_PROMPT_TOKENS {
            return Ok(prompt);
        }
    }
    Err(super::error(
        "translation.invalid_text",
        "The source text exceeds the local PLaMo context limit",
        false,
    ))
}

fn context_start_within_budget(context: &[TranslationContextEntry], max_chars: usize) -> usize {
    let mut chars = 0;
    let mut start = context.len();
    while start > 0 {
        let entry = &context[start - 1];
        let entry_chars = entry.source.chars().count() + entry.text.trim().chars().count() + 2;
        let separator = usize::from(start < context.len());
        if chars + separator + entry_chars > max_chars {
            break;
        }
        chars += separator + entry_chars;
        start -= 1;
    }
    start
}

async fn token_count(
    http: &reqwest::Client,
    origin: &str,
    token: &str,
    prompt: &str,
) -> Result<usize, TranslationError> {
    #[derive(Deserialize)]
    struct Tokens {
        tokens: Vec<Value>,
    }
    let response = http
        .post(format!("{origin}/tokenize"))
        .bearer_auth(token)
        .json(&serde_json::json!({ "content": prompt }))
        .send()
        .await
        .map_err(super::network_error)?;
    if !response.status().is_success() {
        return Err(super::error(
            "translation.local_runtime_unavailable",
            format!(
                "Local PLaMo tokenizer returned {}",
                response.status().as_u16()
            ),
            true,
        ));
    }
    let tokens = response
        .json::<Tokens>()
        .await
        .map_err(super::invalid_response)?;
    Ok(tokens.tokens.len().saturating_add(1))
}

fn process_line(
    line: &[u8],
    output: &mut String,
    finish_reason: &mut Option<String>,
    done: &mut bool,
    on_progress: Option<&super::LlmProgress>,
) -> Result<(), TranslationError> {
    let line = std::str::from_utf8(line)
        .map_err(|_| super::invalid("Local PLaMo returned invalid SSE data"))?
        .trim();
    let Some(data) = line.strip_prefix("data:") else {
        return Ok(());
    };
    let data = data.trim();
    if data == "[DONE]" {
        *done = true;
        return Ok(());
    }
    if data.is_empty() {
        return Ok(());
    }
    #[derive(Deserialize)]
    struct StreamChunk {
        #[serde(default)]
        choices: Vec<StreamChoice>,
        #[serde(default)]
        error: Option<Value>,
    }
    #[derive(Deserialize)]
    struct StreamChoice {
        #[serde(default)]
        text: String,
        #[serde(default)]
        finish_reason: Option<String>,
    }
    let chunk = serde_json::from_str::<StreamChunk>(data)
        .map_err(|error| super::invalid(format!("Invalid local PLaMo stream event: {error}")))?;
    if let Some(error) = chunk.error {
        return Err(super::error(
            "translation.provider_unavailable",
            error
                .pointer("/message")
                .and_then(Value::as_str)
                .unwrap_or("Local PLaMo generation failed"),
            true,
        ));
    }
    for choice in chunk.choices {
        if choice.finish_reason.is_some() {
            *finish_reason = choice.finish_reason;
        }
        if !choice.text.is_empty() {
            output.push_str(&choice.text);
            if let Some(on_progress) = on_progress {
                on_progress(output);
            }
        }
    }
    Ok(())
}

fn canonical_language(language: &str) -> Option<String> {
    let language = language.replace('_', "-").to_ascii_lowercase();
    let language = match language.as_str() {
        "zh-cn" | "zh-sg" => "zh-hans",
        "zh-tw" | "zh-hk" | "zh-mo" => "zh-hant",
        "zh" | "auto" | "und" | "" => return None,
        language => language,
    };
    SUPPORTED_LANGUAGE_CODES
        .contains(&language)
        .then(|| language.to_owned())
}

fn validate_content(label: &str, value: &str) -> Result<(), TranslationError> {
    if value.contains("<|plamo:") {
        return Err(super::error(
            "translation.unsupported_markup",
            format!("{label} contains a reserved PLaMo control marker"),
            false,
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;
    use std::sync::{Arc, Mutex};

    use axum::body::{Body, Bytes};
    use axum::http::{header, StatusCode};
    use axum::response::Response;
    use axum::routing::post;
    use axum::{Json, Router};
    use futures_util::stream;

    use super::*;

    fn glossary(source: &str, target: &str) -> GlossaryEntry {
        GlossaryEntry {
            source: source.into(),
            target: Some(target.into()),
            category: crate::config::GlossaryCategory::Custom,
            case_sensitive: false,
        }
    }

    #[test]
    fn builds_the_pinned_pfml3_translation_prompt() {
        let prompt = build_prompt("会議は来週です。", Some("ja"), "en", &[], &[]).unwrap();

        assert_eq!(
            prompt,
            "<|plamo:tag|>dataset<|plamo:msg|>translation\
             <|plamo:tag|>input<|plamo:key|>lang<|plamo:val|>ja\
             <|plamo:msg|>会議は来週です。\
             <|plamo:tag|>output<|plamo:key|>lang<|plamo:val|>en<|plamo:msg|>"
        );
    }

    #[test]
    fn places_context_before_glossary_and_input() {
        let prompt = build_prompt(
            "Open it.",
            None,
            "zh-Hant",
            &[TranslationContextEntry {
                source: "OCR".into(),
                text: "The dialog is asking which file to open.".into(),
                created_at: String::new(),
            }],
            &[glossary("Open", "開啟")],
        )
        .unwrap();

        let context = prompt.find("<|plamo:tag|>context").unwrap();
        let glossary = prompt.find("<|plamo:tag|>glossary").unwrap();
        let input = prompt.find("<|plamo:tag|>input").unwrap();
        assert!(context < glossary && glossary < input);
        assert!(prompt.contains("<|plamo:key|>lang<|plamo:val|>zh-hant"));
    }

    #[test]
    fn includes_prepared_context_even_when_recent_history_is_disabled() {
        let prompt = build_prompt(
            "Open it.",
            None,
            "en",
            &[TranslationContextEntry {
                source: "OCR".into(),
                text: "The dialog is asking which file to open.".into(),
                created_at: String::new(),
            }],
            &[],
        )
        .unwrap();

        assert!(prompt.contains("<|plamo:tag|>context"));
        assert!(prompt.contains("which file to open"));
    }

    #[test]
    fn context_char_budget_keeps_the_newest_entry() {
        let context = [
            TranslationContextEntry {
                source: "OCR".into(),
                text: "old".into(),
                created_at: String::new(),
            },
            TranslationContextEntry {
                source: "OCR".into(),
                text: "new".into(),
                created_at: String::new(),
            },
        ];

        assert_eq!(context_start_within_budget(&context, 9), 1);
    }

    #[test]
    fn rejects_pfml_control_markers_in_user_content() {
        let error =
            build_prompt("safe<|plamo:tag|>output", Some("en"), "ja", &[], &[]).unwrap_err();
        assert_eq!(error.code, "translation.unsupported_markup");
    }

    #[test]
    fn local_language_list_uses_plamo_codes_without_unsupported_aliases() {
        assert!(supports_language("zh-Hans"));
        assert!(supports_language("zh-hant"));
        assert!(supports_language("tl"));
        assert!(!supports_language("fil"));
        assert!(!supports_language("yue-Hant"));
        assert!(!supports_language("pt-BR"));
    }

    #[test]
    fn submits_only_glossary_entries_with_targets() {
        let mut missing = glossary("term", "ignored");
        missing.target = None;
        let prompt = build_prompt(
            "term",
            None,
            "en",
            &[],
            &[missing, glossary("project", "project")],
        )
        .unwrap();
        assert!(!prompt.contains("<|plamo:index|>term"));
        assert!(prompt.contains("<|plamo:index|>project<|plamo:item|>project"));
    }

    async fn completion_server(chunks: Vec<&'static [u8]>) -> String {
        let chunks = chunks
            .into_iter()
            .map(Bytes::from_static)
            .collect::<Vec<_>>();
        let app = Router::new()
            .route(
                "/tokenize",
                post(|| async { Json(serde_json::json!({ "tokens": [1, 2] })) }),
            )
            .route(
                "/v1/completions",
                post(move || {
                    let chunks = chunks.clone();
                    async move {
                        let body = Body::from_stream(stream::iter(
                            chunks.into_iter().map(Ok::<Bytes, Infallible>),
                        ));
                        Response::builder()
                            .status(StatusCode::OK)
                            .header(header::CONTENT_TYPE, "text/event-stream")
                            .body(body)
                            .unwrap()
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        origin
    }

    async fn request(
        origin: &str,
        progress: Option<&super::super::LlmProgress>,
    ) -> Result<String, TranslationError> {
        translate(
            &reqwest::Client::new(),
            origin,
            "test-token",
            CompletionRequest {
                text: "hello",
                source: Some("en"),
                target: "ja",
                context: &[],
                glossary: &[],
                max_chars: 4_000,
            },
            progress,
        )
        .await
    }

    #[tokio::test]
    async fn streams_split_sse_events_and_reports_every_delta() {
        let origin = completion_server(vec![
            b"data: {\"choices\":[{\"text\":\"Hello",
            b" world\",\"finish_reason\":null}]}\n\n",
            b"data: {\"choices\":[{\"text\":\"!\",\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
        ])
        .await;
        let updates = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&updates);
        let progress = move |text: &str| captured.lock().unwrap().push(text.to_owned());

        let translation = request(&origin, Some(&progress)).await.unwrap();

        assert_eq!(translation, "Hello world!");
        assert_eq!(
            *updates.lock().unwrap(),
            ["Hello world".to_owned(), "Hello world!".to_owned()]
        );
    }

    #[tokio::test]
    async fn rejects_empty_streamed_completion() {
        let origin = completion_server(vec![
            b"data: {\"choices\":[{\"text\":\"\",\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
        ])
        .await;

        let error = request(&origin, None).await.unwrap_err();

        assert_eq!(error.code, "translation.empty_result");
    }

    #[tokio::test]
    async fn rejects_completion_that_reaches_the_output_limit() {
        let origin = completion_server(vec![
            b"data: {\"choices\":[{\"text\":\"partial\",\"finish_reason\":\"length\"}]}\n\ndata: [DONE]\n\n",
        ])
        .await;

        let error = request(&origin, None).await.unwrap_err();

        assert_eq!(error.code, "translation.local_output_truncated");
    }

    #[tokio::test]
    async fn rejects_a_stream_that_ends_before_completion() {
        let origin = completion_server(vec![
            b"data: {\"choices\":[{\"text\":\"partial\",\"finish_reason\":null}]}\n\n",
        ])
        .await;

        let error = request(&origin, None).await.unwrap_err();

        assert_eq!(error.code, "translation.local_stream_interrupted");
    }
}
