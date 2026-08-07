use std::collections::HashMap;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::config::AsrConfig;

use super::read_credential;

mod provider;

use provider::{InitializationEvent, Provider};

#[cfg(test)]
use tokio_tungstenite::tungstenite::http::Request;

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
    let provider = Provider::from_config(&config)?;
    let credential_name = provider.credential_name();
    let key = read_credential(credential_name)?
        .ok_or_else(|| format!("{credential_name} API Key 尚未配置"))?;
    let (socket, task_id) = connect_initialized(provider, &config, silence_seconds, &key).await?;
    let (audio_tx, audio_rx) = mpsc::channel(32);
    let (event_tx, event_rx) = mpsc::channel(32);
    let (stop_tx, stop_rx) = watch::channel(false);
    let task_config = config.clone();
    let task = tokio::spawn(async move {
        run_with_reconnect(
            provider,
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
    let provider = Provider::from_config(&test_config)?;
    let (mut socket, task_id) = connect_initialized(provider, &test_config, 0.4, &key).await?;
    let (events, _) = mpsc::channel(1);
    finish(
        provider,
        &mut socket,
        &test_config,
        task_id.as_deref(),
        &mut HashMap::new(),
        &events,
    )
    .await;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run_with_reconnect(
    provider: Provider,
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
            provider,
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
        match connect_initialized(provider, &config, silence_seconds, &key).await {
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
    provider: Provider,
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
                    let _ = send_audio(provider, socket, packet).await;
                }
                finish(provider, socket, config, task_id, &mut transcripts, events).await;
                return Ok(());
            }
            samples = audio.recv() => {
                let Some(samples) = samples else {
                    if !audio_buffer.is_empty() {
                        let packet = std::mem::take(&mut audio_buffer);
                        let _ = send_audio(provider, socket, packet).await;
                    }
                    finish(provider, socket, config, task_id, &mut transcripts, events).await;
                    return Ok(());
                };
                audio_buffer.extend(samples);
                while audio_buffer.len() >= 1600 {
                    let packet = audio_buffer.drain(..1600).collect::<Vec<_>>();
                    send_audio(provider, socket, packet).await?;
                }
            }
            message = socket.next() => {
                let message = message
                    .ok_or_else(|| "云端识别连接已关闭".to_string())?
                    .map_err(|error| format!("读取云端识别事件失败：{error}"))?;
                match message {
                    Message::Text(text) => {
                        if let Some(event) = normalize_event(provider, config, &text, &mut transcripts)? {
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

async fn connect(provider: Provider, config: &AsrConfig, key: &str) -> Result<Socket, String> {
    let request = provider.build_request(config, key)?;
    tokio_tungstenite::connect_async(request)
        .await
        .map(|(socket, _)| socket)
        .map_err(|error| format!("无法连接云端识别服务：{error}"))
}

async fn connect_initialized(
    provider: Provider,
    config: &AsrConfig,
    silence_seconds: f64,
    key: &str,
) -> Result<(Socket, Option<String>), String> {
    let mut socket = tokio::time::timeout(Duration::from_secs(10), connect(provider, config, key))
        .await
        .map_err(|_| "连接云端识别服务超时".to_string())??;
    let task_id = provider.task_id();
    let update = provider.start_message(config, silence_seconds, task_id.as_deref());
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
                    match provider.initialization_event(&value) {
                        InitializationEvent::Ready => return Ok(()),
                        InitializationEvent::Failed(detail) => return Err(detail),
                        InitializationEvent::Pending => {}
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

fn normalize_event(
    provider: Provider,
    config: &AsrConfig,
    message: &str,
    transcripts: &mut HashMap<String, String>,
) -> Result<Option<CloudEvent>, String> {
    let value: Value =
        serde_json::from_str(message).map_err(|error| format!("云端识别返回无效 JSON：{error}"))?;
    provider.normalize_event(config, &value, transcripts)
}

async fn send_audio(
    provider: Provider,
    socket: &mut Socket,
    samples: Vec<f32>,
) -> Result<(), String> {
    socket
        .send(provider.audio_message(&samples))
        .await
        .map_err(|error| format!("发送云端音频失败：{error}"))
}

async fn finish(
    provider: Provider,
    socket: &mut Socket,
    config: &AsrConfig,
    task_id: Option<&str>,
    transcripts: &mut HashMap<String, String>,
    events: &mpsc::Sender<CloudEvent>,
) {
    let _ = socket.send(provider.finish_message(task_id)).await;
    let deadline = tokio::time::sleep(Duration::from_secs(3));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => break,
            message = socket.next() => {
                let Some(Ok(Message::Text(text))) = message else { break };
                let finished = serde_json::from_str::<Value>(&text)
                    .ok()
                    .is_some_and(|value| provider.is_finished(&value));
                if let Ok(Some(event)) = normalize_event(provider, config, &text, transcripts) {
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
fn build_request(config: &AsrConfig, key: &str) -> Result<Request<()>, String> {
    Provider::from_config(config)?.build_request(config, key)
}

#[cfg(test)]
fn session_update(config: &AsrConfig, silence_seconds: f64) -> Value {
    Provider::from_config(config)
        .unwrap()
        .start_message(config, silence_seconds, None)
}

#[cfg(test)]
fn fun_run_task(config: &AsrConfig, silence_seconds: f64, task_id: &str) -> Value {
    Provider::FunAsr.start_message(config, silence_seconds, Some(task_id))
}

#[cfg(test)]
fn pcm16_bytes(samples: &[f32]) -> Vec<u8> {
    provider::pcm16_bytes(samples)
}

#[cfg(test)]
fn resample_16k_to_24k(samples: &[f32]) -> Vec<f32> {
    provider::resample_16k_to_24k(samples)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn normalize(
        config: &AsrConfig,
        message: &str,
        transcripts: &mut HashMap<String, String>,
    ) -> Result<Option<CloudEvent>, String> {
        normalize_event(Provider::from_config(config)?, config, message, transcripts)
    }

    #[test]
    fn openai_deltas_are_accumulated_and_completed() {
        let config = AsrConfig {
            backend: "openai_realtime".into(),
            ..AsrConfig::default()
        };
        let mut transcripts = HashMap::new();
        let first = normalize(&config, r#"{"type":"conversation.item.input_audio_transcription.delta","item_id":"a","delta":"hel"}"#, &mut transcripts).unwrap();
        let second = normalize(&config, r#"{"type":"conversation.item.input_audio_transcription.delta","item_id":"a","delta":"lo"}"#, &mut transcripts).unwrap();
        let final_event = normalize(&config, r#"{"type":"conversation.item.input_audio_transcription.completed","item_id":"a","transcript":"hello"}"#, &mut transcripts).unwrap();
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
        let event = normalize(
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
        assert_eq!(request.uri().to_string(), "wss://ws-example.cn-beijing.maas.aliyuncs.com/api-ws/v1/realtime?model=qwen3-asr-flash-realtime");
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
        let partial = normalize(&config, r#"{"header":{"event":"result-generated"},"payload":{"output":{"sentence":{"sentence_id":1,"text":"你好","sentence_end":false}}}}"#, &mut transcripts).unwrap();
        let final_event = normalize(&config, r#"{"header":{"event":"result-generated"},"payload":{"output":{"sentence":{"sentence_id":1,"text":"你好世界","sentence_end":true}}}}"#, &mut transcripts).unwrap();
        let heartbeat = normalize(&config, r#"{"header":{"event":"result-generated"},"payload":{"output":{"sentence":{"heartbeat":true,"text":""}}}}"#, &mut transcripts).unwrap();
        let failure = normalize(&config, r#"{"header":{"event":"task-failed","error_code":"InvalidParameter","error_message":"bad request"}}"#, &mut transcripts).unwrap();
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
