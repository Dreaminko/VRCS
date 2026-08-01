use std::collections::HashMap;
use std::time::Duration;

use base64::Engine as _;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::http::Request;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::config::AsrConfig;

use super::read_credential;

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

#[derive(Debug, Clone, PartialEq)]
pub enum CloudEvent {
    Partial {
        utterance_id: String,
        text: String,
        language: Option<String>,
    },
    Final {
        utterance_id: String,
        text: String,
        language: Option<String>,
    },
    Failed {
        code: String,
        detail: String,
    },
}

pub struct StreamingSession {
    audio: mpsc::Sender<Vec<f32>>,
    events: mpsc::Receiver<CloudEvent>,
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl StreamingSession {
    pub async fn send(&self, samples: Vec<f32>) -> Result<(), String> {
        self.audio
            .send(samples)
            .await
            .map_err(|_| "云端识别会话已关闭".to_string())
    }

    pub async fn recv(&mut self) -> Option<CloudEvent> {
        self.events.recv().await
    }

    pub async fn stop(self) {
        let _ = self.stop.send(true);
        let _ = self.task.await;
    }

    pub async fn stop_and_drain(mut self) -> Vec<CloudEvent> {
        let _ = self.stop.send(true);
        let _ = self.task.await;
        let mut drained = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            drained.push(event);
        }
        drained
    }
}

pub async fn spawn_streaming_session(
    config: AsrConfig,
    silence_seconds: f64,
) -> Result<StreamingSession, String> {
    let provider = provider(&config)?;
    let key = read_credential(provider)?.ok_or_else(|| format!("{provider} API Key 尚未配置"))?;
    let (socket, task_id) = connect_initialized(&config, silence_seconds, &key).await?;
    let (audio_tx, audio_rx) = mpsc::channel(32);
    let (event_tx, event_rx) = mpsc::channel(32);
    let (stop_tx, stop_rx) = watch::channel(false);
    let task_config = config.clone();
    let task = tokio::spawn(async move {
        run_with_reconnect(
            task_config,
            silence_seconds,
            key,
            socket,
            task_id,
            audio_rx,
            event_tx,
            stop_rx,
        )
        .await;
    });
    Ok(StreamingSession {
        audio: audio_tx,
        events: event_rx,
        stop: stop_tx,
        task,
    })
}

fn provider(config: &AsrConfig) -> Result<&'static str, String> {
    match config.backend.as_str() {
        "qwen_realtime" | "fun_asr_realtime" => Ok("qwen"),
        "openai_realtime" => Ok("openai"),
        other => Err(format!("后端 {other} 不是云端实时识别后端")),
    }
}

pub async fn test_streaming_connection(
    config: &AsrConfig,
    provider_name: &str,
) -> Result<(), String> {
    let key = read_credential(provider_name)?
        .ok_or_else(|| format!("{provider_name} API Key 尚未配置"))?;
    let mut test_config = config.clone();
    test_config.backend = match provider_name {
        "qwen" if config.backend == "fun_asr_realtime" => "fun_asr_realtime",
        "qwen" => "qwen_realtime",
        "openai" => "openai_realtime",
        _ => return Err(format!("不支持的云端识别服务：{provider_name}")),
    }
    .into();
    let (mut socket, task_id) = connect_initialized(&test_config, 0.4, &key).await?;
    let (events, _) = mpsc::channel(1);
    finish(
        &mut socket,
        &test_config,
        task_id.as_deref(),
        &mut HashMap::new(),
        &events,
    )
    .await;
    Ok(())
}

async fn run_with_reconnect(
    config: AsrConfig,
    silence_seconds: f64,
    key: String,
    mut socket: Socket,
    mut task_id: Option<String>,
    mut audio: mpsc::Receiver<Vec<f32>>,
    events: mpsc::Sender<CloudEvent>,
    mut stop: watch::Receiver<bool>,
) {
    let mut backoff = Duration::from_millis(500);
    loop {
        let outcome = run_session(
            &config,
            &mut socket,
            task_id.as_deref(),
            &mut audio,
            &events,
            &mut stop,
        )
        .await;
        if *stop.borrow() || audio.is_closed() {
            break;
        }
        let detail = match outcome {
            Ok(()) => "云端识别连接已关闭".to_string(),
            Err(error) => error,
        };
        let _ = events
            .send(CloudEvent::Failed {
                code: "asr.cloud_disconnected".into(),
                detail,
            })
            .await;
        if config.cloud_failure_policy != "reconnect" {
            break;
        }
        tokio::select! {
            _ = tokio::time::sleep(backoff) => {}
            _ = stop.changed() => break,
        }
        match connect_initialized(&config, silence_seconds, &key).await {
            Ok((next_socket, next_task_id)) => {
                socket = next_socket;
                task_id = next_task_id;
                backoff = Duration::from_millis(500);
            }
            Err(error) => {
                let _ = events
                    .send(CloudEvent::Failed {
                        code: "asr.cloud_reconnect_failed".into(),
                        detail: error,
                    })
                    .await;
                backoff = (backoff * 2).min(Duration::from_secs(8));
            }
        }
    }
}

async fn run_session(
    config: &AsrConfig,
    socket: &mut Socket,
    task_id: Option<&str>,
    audio: &mut mpsc::Receiver<Vec<f32>>,
    events: &mpsc::Sender<CloudEvent>,
    stop: &mut watch::Receiver<bool>,
) -> Result<(), String> {
    let mut transcripts = HashMap::new();
    let mut audio_buffer = Vec::with_capacity(2048);
    loop {
        tokio::select! {
            _ = stop.changed() => {
                if !audio_buffer.is_empty() {
                    let packet = std::mem::take(&mut audio_buffer);
                    let _ = send_audio(socket, config, packet).await;
                }
                finish(socket, config, task_id, &mut transcripts, events).await;
                return Ok(());
            }
            samples = audio.recv() => {
                let Some(samples) = samples else {
                    if !audio_buffer.is_empty() {
                        let packet = std::mem::take(&mut audio_buffer);
                        let _ = send_audio(socket, config, packet).await;
                    }
                    finish(socket, config, task_id, &mut transcripts, events).await;
                    return Ok(());
                };
                audio_buffer.extend(samples);
                while audio_buffer.len() >= 1600 {
                    let packet = audio_buffer.drain(..1600).collect::<Vec<_>>();
                    send_audio(socket, config, packet).await?;
                }
            }
            message = socket.next() => {
                let message = message
                    .ok_or_else(|| "云端识别连接已关闭".to_string())?
                    .map_err(|error| format!("读取云端识别事件失败：{error}"))?;
                match message {
                    Message::Text(text) => {
                        if let Some(event) = normalize_event(config, &text, &mut transcripts)? {
                            if events.send(event).await.is_err() {
                                return Ok(());
                            }
                        }
                    }
                    Message::Close(_) => return Err("云端识别服务关闭了连接".into()),
                    Message::Ping(value) => socket.send(Message::Pong(value)).await
                        .map_err(|error| format!("云端识别心跳失败：{error}"))?,
                    _ => {}
                }
            }
        }
    }
}

async fn connect(config: &AsrConfig, key: &str) -> Result<Socket, String> {
    let request = build_request(config, key)?;
    tokio_tungstenite::connect_async(request)
        .await
        .map(|(socket, _)| socket)
        .map_err(|error| format!("无法连接云端识别服务：{error}"))
}

fn build_request(config: &AsrConfig, key: &str) -> Result<Request<()>, String> {
    let url = match config.backend.as_str() {
        "qwen_realtime" | "fun_asr_realtime" => {
            let workspace = config.qwen.workspace_id.trim();
            if workspace.is_empty() {
                return Err("阿里云 Workspace ID 尚未配置".into());
            }
            let region = match config.qwen.region.as_str() {
                "singapore" => "ap-southeast-1",
                "china_beijing" => "cn-beijing",
                other => return Err(format!("不支持的阿里云区域：{other}")),
            };
            if config.backend == "fun_asr_realtime" {
                format!("wss://{workspace}.{region}.maas.aliyuncs.com/api-ws/v1/inference")
            } else {
                format!(
                    "wss://{}.{}.maas.aliyuncs.com/api-ws/v1/realtime?model={}",
                    workspace, region, config.qwen.model
                )
            }
        }
        "openai_realtime" => "wss://api.openai.com/v1/realtime?intent=transcription".into(),
        other => return Err(format!("后端 {other} 不是云端实时识别后端")),
    };
    let mut request = url
        .into_client_request()
        .map_err(|error| format!("云端识别地址无效：{error}"))?;
    request.headers_mut().insert(
        "Authorization",
        HeaderValue::from_str(&format!("Bearer {key}"))
            .map_err(|_| "API Key 包含无效字符".to_string())?,
    );
    if config.backend != "fun_asr_realtime" {
        request
            .headers_mut()
            .insert("OpenAI-Beta", HeaderValue::from_static("realtime=v1"));
    }
    Ok(request)
}

async fn connect_initialized(
    config: &AsrConfig,
    silence_seconds: f64,
    key: &str,
) -> Result<(Socket, Option<String>), String> {
    let mut socket = tokio::time::timeout(Duration::from_secs(10), connect(config, key))
        .await
        .map_err(|_| "连接云端识别服务超时".to_string())??;
    let task_id = (config.backend == "fun_asr_realtime").then(|| uuid::Uuid::new_v4().to_string());
    let update = task_id
        .as_deref()
        .map(|task_id| fun_run_task(config, silence_seconds, task_id))
        .unwrap_or_else(|| session_update(config, silence_seconds));
    socket
        .send(Message::Text(update.to_string().into()))
        .await
        .map_err(|error| format!("无法初始化云端识别会话：{error}"))?;
    let configured = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let message = socket
                .next()
                .await
                .ok_or_else(|| "云端识别连接在初始化时关闭".to_string())?
                .map_err(|error| format!("读取云端识别初始化事件失败：{error}"))?;
            match message {
                Message::Text(text) => {
                    let value: Value = serde_json::from_str(&text)
                        .map_err(|error| format!("云端识别返回无效 JSON：{error}"))?;
                    let kind = if config.backend == "fun_asr_realtime" {
                        value.pointer("/header/event").and_then(Value::as_str)
                    } else {
                        value.get("type").and_then(Value::as_str)
                    };
                    match kind {
                        Some("session.updated" | "task-started") => return Ok(()),
                        Some("error" | "task-failed") => {
                            let detail = value
                                .pointer("/error/message")
                                .or_else(|| value.pointer("/header/error_message"))
                                .and_then(Value::as_str)
                                .unwrap_or("云端识别会话配置失败");
                            return Err(detail.to_string());
                        }
                        _ => {}
                    }
                }
                Message::Ping(value) => socket
                    .send(Message::Pong(value))
                    .await
                    .map_err(|error| format!("云端识别心跳失败：{error}"))?,
                Message::Close(_) => return Err("云端识别服务关闭了初始化连接".into()),
                _ => {}
            }
        }
    })
    .await
    .map_err(|_| "等待云端识别会话确认超时".to_string())?;
    configured?;
    Ok((socket, task_id))
}

fn fun_run_task(config: &AsrConfig, silence_seconds: f64, task_id: &str) -> Value {
    let mut parameters = json!({
        "format": "pcm",
        "sample_rate": 16000,
        "semantic_punctuation_enabled": false,
        "max_sentence_silence": (silence_seconds * 1000.0).round() as u64,
    });
    if config.language != "auto" {
        parameters["language_hints"] = json!([config.language]);
    }
    let context = config.fun_asr.context.trim();
    let input = if context.is_empty() {
        json!({})
    } else {
        json!({
            "context": [{
                "role": "user",
                "content": [{ "type": "input_text", "text": context }]
            }]
        })
    };
    json!({
        "header": {
            "action": "run-task",
            "task_id": task_id,
            "streaming": "duplex"
        },
        "payload": {
            "task_group": "audio",
            "task": "asr",
            "function": "recognition",
            "model": config.fun_asr.model,
            "parameters": parameters,
            "input": input
        }
    })
}

fn session_update(config: &AsrConfig, silence_seconds: f64) -> Value {
    let language = (config.language != "auto").then(|| config.language.clone());
    let transcription_language = language
        .map(|language| json!({ "language": language }))
        .unwrap_or_else(|| json!({}));
    let mut qwen_transcription = transcription_language.clone();
    if !config.qwen.context.trim().is_empty() {
        qwen_transcription["corpus"] = json!({ "text": config.qwen.context.trim() });
    }
    let mut openai_transcription = json!({ "model": config.openai.model });
    if let Some(language) = transcription_language.get("language") {
        openai_transcription["language"] = language.clone();
    }
    let silence_ms = (silence_seconds * 1000.0).round() as u64;
    if config.backend == "qwen_realtime" {
        json!({
            "event_id": uuid::Uuid::new_v4().to_string(),
            "type": "session.update",
            "session": {
                "input_audio_format": "pcm",
                "sample_rate": 16000,
                "input_audio_transcription": qwen_transcription,
                "turn_detection": {
                    "type": "server_vad",
                    "threshold": 0.0,
                    "silence_duration_ms": silence_ms,
                }
            }
        })
    } else {
        json!({
            "type": "session.update",
            "session": {
                "type": "transcription",
                "audio": { "input": {
                    "format": { "type": "audio/pcm", "rate": 24000 },
                    "transcription": openai_transcription,
                    "turn_detection": {
                        "type": "server_vad",
                        "prefix_padding_ms": 200,
                        "silence_duration_ms": silence_ms,
                    }
                }}
            }
        })
    }
}

fn normalize_event(
    config: &AsrConfig,
    message: &str,
    transcripts: &mut HashMap<String, String>,
) -> Result<Option<CloudEvent>, String> {
    let value: Value =
        serde_json::from_str(message).map_err(|error| format!("云端识别返回无效 JSON：{error}"))?;
    if config.backend == "fun_asr_realtime" {
        return normalize_fun_event(config, &value);
    }
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if kind == "error" || kind.ends_with(".failed") {
        let detail = value
            .pointer("/error/message")
            .or_else(|| value.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("云端识别请求失败");
        return Ok(Some(CloudEvent::Failed {
            code: "asr.cloud_error".into(),
            detail: detail.into(),
        }));
    }

    let id = value
        .get("item_id")
        .or_else(|| value.get("utterance_id"))
        .or_else(|| value.get("event_id"))
        .and_then(Value::as_str)
        .unwrap_or("current")
        .to_string();
    let language = value
        .get("language")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| (config.language != "auto").then(|| config.language.clone()));

    if kind.ends_with(".delta") {
        let delta = value
            .get("delta")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let text = transcripts.entry(id.clone()).or_default();
        text.push_str(delta);
        return Ok((!text.is_empty()).then(|| CloudEvent::Partial {
            utterance_id: id,
            text: text.clone(),
            language,
        }));
    }
    if kind.ends_with(".completed") {
        let text = value
            .get("transcript")
            .or_else(|| value.get("text"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| transcripts.remove(&id))
            .unwrap_or_default();
        transcripts.remove(&id);
        return Ok(Some(CloudEvent::Final {
            utterance_id: id,
            text,
            language,
        }));
    }
    if kind.ends_with(".text") {
        let stable = value
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let draft = value
            .get("stash")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let text = format!("{stable}{draft}");
        return Ok((!text.is_empty()).then(|| CloudEvent::Partial {
            utterance_id: id,
            text,
            language,
        }));
    }
    Ok(None)
}

fn normalize_fun_event(config: &AsrConfig, value: &Value) -> Result<Option<CloudEvent>, String> {
    let kind = value
        .pointer("/header/event")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if kind == "task-failed" {
        return Ok(Some(CloudEvent::Failed {
            code: value
                .pointer("/header/error_code")
                .and_then(Value::as_str)
                .unwrap_or("asr.cloud_error")
                .to_string(),
            detail: value
                .pointer("/header/error_message")
                .and_then(Value::as_str)
                .unwrap_or("Fun-ASR 请求失败")
                .to_string(),
        }));
    }
    if kind != "result-generated" {
        return Ok(None);
    }
    let Some(sentence) = value.pointer("/payload/output/sentence") else {
        return Ok(None);
    };
    if sentence
        .get("heartbeat")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Ok(None);
    }
    let text = sentence
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if text.is_empty() {
        return Ok(None);
    }
    let utterance_id = sentence
        .get("sentence_id")
        .map(|id| {
            id.as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| id.to_string())
        })
        .unwrap_or_else(|| "current".into());
    let language = (config.language != "auto").then(|| config.language.clone());
    if sentence
        .get("sentence_end")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        Ok(Some(CloudEvent::Final {
            utterance_id,
            text,
            language,
        }))
    } else {
        Ok(Some(CloudEvent::Partial {
            utterance_id,
            text,
            language,
        }))
    }
}

fn pcm16_bytes(samples: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for sample in samples {
        bytes.extend_from_slice(&((sample.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    bytes
}

fn pcm16_base64(samples: &[f32]) -> String {
    base64::engine::general_purpose::STANDARD.encode(pcm16_bytes(samples))
}

async fn send_audio(
    socket: &mut Socket,
    config: &AsrConfig,
    samples: Vec<f32>,
) -> Result<(), String> {
    if config.backend == "fun_asr_realtime" {
        return socket
            .send(Message::Binary(pcm16_bytes(&samples).into()))
            .await
            .map_err(|error| format!("发送云端音频失败：{error}"));
    }
    let samples = if config.backend == "openai_realtime" {
        resample_16k_to_24k(&samples)
    } else {
        samples
    };
    let payload = json!({
        "type": "input_audio_buffer.append",
        "audio": pcm16_base64(&samples),
    });
    let mut payload = payload;
    if config.backend == "qwen_realtime" {
        payload["event_id"] = json!(uuid::Uuid::new_v4().to_string());
    }
    socket
        .send(Message::Text(payload.to_string().into()))
        .await
        .map_err(|error| format!("发送云端音频失败：{error}"))
}

fn resample_16k_to_24k(samples: &[f32]) -> Vec<f32> {
    if samples.is_empty() {
        return Vec::new();
    }
    let output_len = samples.len() * 3 / 2;
    (0..output_len)
        .map(|index| {
            let position = index as f32 * 2.0 / 3.0;
            let left = position.floor() as usize;
            let fraction = position - left as f32;
            let right = (left + 1).min(samples.len() - 1);
            samples[left] + (samples[right] - samples[left]) * fraction
        })
        .collect()
}

async fn finish(
    socket: &mut Socket,
    config: &AsrConfig,
    task_id: Option<&str>,
    transcripts: &mut HashMap<String, String>,
    events: &mpsc::Sender<CloudEvent>,
) {
    if config.backend == "fun_asr_realtime" {
        if let Some(task_id) = task_id {
            let _ = socket
                .send(Message::Text(
                    json!({
                        "header": { "action": "finish-task", "task_id": task_id },
                        "payload": { "input": {} }
                    })
                    .to_string()
                    .into(),
                ))
                .await;
        }
    } else if config.backend == "qwen_realtime" {
        let _ = socket
            .send(Message::Text(
                json!({
                    "event_id": uuid::Uuid::new_v4().to_string(),
                    "type": "session.finish"
                })
                .to_string()
                .into(),
            ))
            .await;
    } else {
        let _ = socket
            .send(Message::Text(
                json!({ "type": "input_audio_buffer.commit" })
                    .to_string()
                    .into(),
            ))
            .await;
    }
    let deadline = tokio::time::sleep(Duration::from_secs(3));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => break,
            message = socket.next() => {
                let Some(Ok(Message::Text(text))) = message else { break };
                let finished = serde_json::from_str::<Value>(&text).ok().is_some_and(|value| {
                    value.get("type").and_then(Value::as_str) == Some("session.finished")
                        || value.pointer("/header/event").and_then(Value::as_str) == Some("task-finished")
                });
                if let Ok(Some(event)) = normalize_event(config, &text, transcripts) {
                    let _ = events.send(event).await;
                }
                if finished {
                    break;
                }
            }
        }
    }
    let _ = socket.close(None).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_deltas_are_accumulated_and_completed() {
        let config = AsrConfig {
            backend: "openai_realtime".into(),
            ..AsrConfig::default()
        };
        let mut transcripts = HashMap::new();
        let first = normalize_event(&config, r#"{"type":"conversation.item.input_audio_transcription.delta","item_id":"a","delta":"hel"}"#, &mut transcripts).unwrap();
        let second = normalize_event(&config, r#"{"type":"conversation.item.input_audio_transcription.delta","item_id":"a","delta":"lo"}"#, &mut transcripts).unwrap();
        let final_event = normalize_event(&config, r#"{"type":"conversation.item.input_audio_transcription.completed","item_id":"a","transcript":"hello"}"#, &mut transcripts).unwrap();
        assert!(matches!(first, Some(CloudEvent::Partial { text, .. }) if text == "hel"));
        assert!(matches!(second, Some(CloudEvent::Partial { text, .. }) if text == "hello"));
        assert!(matches!(final_event, Some(CloudEvent::Final { text, .. }) if text == "hello"));
    }

    #[test]
    fn resampler_produces_24khz_length() {
        assert_eq!(resample_16k_to_24k(&vec![0.0; 1600]).len(), 2400);
    }

    #[test]
    fn qwen_partial_combines_stable_and_draft_text() {
        let config = AsrConfig {
            backend: "qwen_realtime".into(),
            ..AsrConfig::default()
        };
        let mut transcripts = HashMap::new();
        let event = normalize_event(
            &config,
            r#"{"type":"conversation.item.input_audio_transcription.text","item_id":"a","text":"hello ","stash":"world","language":"en"}"#,
            &mut transcripts,
        )
        .unwrap();
        assert!(matches!(event, Some(CloudEvent::Partial { text, .. }) if text == "hello world"));
    }

    #[test]
    fn session_updates_use_provider_sample_rates_and_vad_timeout() {
        let mut qwen = AsrConfig {
            backend: "qwen_realtime".into(),
            ..AsrConfig::default()
        };
        qwen.language = "auto".into();
        qwen.qwen.context = "VRChat, VRCX".into();
        let qwen_update = session_update(&qwen, 0.7);
        assert_eq!(qwen_update["session"]["sample_rate"], 16_000);
        assert_eq!(
            qwen_update["session"]["turn_detection"]["silence_duration_ms"],
            700
        );
        assert_eq!(qwen_update["session"]["turn_detection"]["threshold"], 0.0);
        assert!(qwen_update["session"]["input_audio_transcription"]
            .get("language")
            .is_none());
        assert_eq!(
            qwen_update["session"]["input_audio_transcription"]["corpus"]["text"],
            "VRChat, VRCX"
        );

        let openai = AsrConfig {
            backend: "openai_realtime".into(),
            ..AsrConfig::default()
        };
        let openai_update = session_update(&openai, 0.4);
        assert_eq!(
            openai_update["session"]["audio"]["input"]["format"]["rate"],
            24_000
        );
        assert_eq!(
            openai_update["session"]["audio"]["input"]["transcription"]["model"],
            "gpt-4o-mini-transcribe"
        );
    }

    #[test]
    fn qwen_request_uses_workspace_endpoint_and_realtime_headers() {
        let mut config = AsrConfig {
            backend: "qwen_realtime".into(),
            ..AsrConfig::default()
        };
        config.qwen.workspace_id = "ws-example".into();
        config.qwen.region = "china_beijing".into();

        let request = build_request(&config, "sk-test").unwrap();

        assert_eq!(
            request.uri().to_string(),
            "wss://ws-example.cn-beijing.maas.aliyuncs.com/api-ws/v1/realtime?model=qwen3-asr-flash-realtime"
        );
        assert_eq!(request.headers()["Authorization"], "Bearer sk-test");
        assert_eq!(request.headers()["OpenAI-Beta"], "realtime=v1");
    }

    #[test]
    fn qwen_request_rejects_missing_workspace() {
        let config = AsrConfig {
            backend: "qwen_realtime".into(),
            ..AsrConfig::default()
        };

        assert_eq!(
            build_request(&config, "sk-test").unwrap_err(),
            "阿里云 Workspace ID 尚未配置"
        );
    }

    #[test]
    fn fun_asr_request_uses_inference_endpoint_without_realtime_header() {
        let mut config = AsrConfig {
            backend: "fun_asr_realtime".into(),
            ..AsrConfig::default()
        };
        config.qwen.workspace_id = "ws-example".into();
        config.qwen.region = "singapore".into();

        let request = build_request(&config, "sk-test").unwrap();

        assert_eq!(
            request.uri().to_string(),
            "wss://ws-example.ap-southeast-1.maas.aliyuncs.com/api-ws/v1/inference"
        );
        assert_eq!(request.headers()["Authorization"], "Bearer sk-test");
        assert!(!request.headers().contains_key("OpenAI-Beta"));
    }

    #[test]
    fn fun_asr_run_task_contains_streaming_audio_and_context_options() {
        let mut config = AsrConfig {
            backend: "fun_asr_realtime".into(),
            language: "zh".into(),
            ..AsrConfig::default()
        };
        config.fun_asr.context = "VRChat 专有名词".into();

        let task = fun_run_task(&config, 0.7, "task-1");

        assert_eq!(task["header"]["action"], "run-task");
        assert_eq!(task["header"]["streaming"], "duplex");
        assert_eq!(task["payload"]["model"], "fun-asr-realtime");
        assert_eq!(task["payload"]["parameters"]["format"], "pcm");
        assert_eq!(task["payload"]["parameters"]["sample_rate"], 16_000);
        assert_eq!(task["payload"]["parameters"]["max_sentence_silence"], 700);
        assert_eq!(task["payload"]["parameters"]["language_hints"][0], "zh");
        assert_eq!(
            task["payload"]["input"]["context"][0]["content"][0]["text"],
            "VRChat 专有名词"
        );
    }

    #[test]
    fn fun_asr_results_are_normalized_and_heartbeats_are_ignored() {
        let config = AsrConfig {
            backend: "fun_asr_realtime".into(),
            ..AsrConfig::default()
        };
        let mut transcripts = HashMap::new();
        let partial = normalize_event(
            &config,
            r#"{"header":{"event":"result-generated"},"payload":{"output":{"sentence":{"sentence_id":1,"text":"你好","sentence_end":false}}}}"#,
            &mut transcripts,
        )
        .unwrap();
        let final_event = normalize_event(
            &config,
            r#"{"header":{"event":"result-generated"},"payload":{"output":{"sentence":{"sentence_id":1,"text":"你好世界","sentence_end":true}}}}"#,
            &mut transcripts,
        )
        .unwrap();
        let heartbeat = normalize_event(
            &config,
            r#"{"header":{"event":"result-generated"},"payload":{"output":{"sentence":{"heartbeat":true,"text":""}}}}"#,
            &mut transcripts,
        )
        .unwrap();
        let failure = normalize_event(
            &config,
            r#"{"header":{"event":"task-failed","error_code":"InvalidParameter","error_message":"bad request"}}"#,
            &mut transcripts,
        )
        .unwrap();

        assert!(
            matches!(partial, Some(CloudEvent::Partial { utterance_id, text, .. }) if utterance_id == "1" && text == "你好")
        );
        assert!(matches!(final_event, Some(CloudEvent::Final { text, .. }) if text == "你好世界"));
        assert_eq!(heartbeat, None);
        assert!(
            matches!(failure, Some(CloudEvent::Failed { code, detail }) if code == "InvalidParameter" && detail == "bad request")
        );
        assert_eq!(pcm16_bytes(&[0.0; 1600]).len(), 3200);
    }
}
