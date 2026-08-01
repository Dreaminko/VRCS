use std::sync::{Arc, Mutex};

use tokio::sync::broadcast;

use crate::asr::AsrService;
use crate::db::Database;
use crate::models::{now_iso8601, LiveTranscription, Subtitle};

#[derive(Clone)]
pub(crate) struct PipelineDependencies {
    asr: Arc<Mutex<AsrService>>,
    database: Arc<Mutex<Database>>,
    subtitles: broadcast::Sender<Subtitle>,
    live: broadcast::Sender<LiveTranscription>,
    history_limit: u32,
}

impl PipelineDependencies {
    pub(crate) fn new(
        asr: Arc<Mutex<AsrService>>,
        database: Arc<Mutex<Database>>,
        subtitles: broadcast::Sender<Subtitle>,
        live: broadcast::Sender<LiveTranscription>,
        history_limit: u32,
    ) -> Self {
        Self {
            asr,
            database,
            subtitles,
            live,
            history_limit,
        }
    }

    pub(crate) fn publish_live(&self, event: LiveTranscription) {
        let _ = self.live.send(event);
    }

    pub(crate) async fn transcribe_and_publish(
        &self,
        segment: Vec<f32>,
        source: &'static str,
    ) -> Result<(), String> {
        let rms = (segment.iter().map(|sample| sample * sample).sum::<f32>()
            / segment.len().max(1) as f32)
            .sqrt();
        let peak = segment.iter().copied().map(f32::abs).fold(0.0, f32::max);
        tracing::debug!(
            source,
            samples = segment.len(),
            duration_seconds = segment.len() as f64 / 16_000.0,
            rms,
            peak,
            "sending speech segment to ASR"
        );
        let transcriber = Arc::clone(&self.asr);
        let transcription = tokio::task::spawn_blocking(move || {
            transcriber.lock().expect("asr lock").transcribe(&segment)
        })
        .await
        .map_err(|error| format!("识别任务异常退出：{error}"))??;
        tracing::debug!(
            source,
            text_length = transcription.text.chars().count(),
            language = transcription.language.as_deref().unwrap_or("unknown"),
            "ASR transcription completed"
        );
        self.publish_text(transcription.text, transcription.language, source)
            .await
    }

    pub(crate) async fn publish_text(
        &self,
        text: String,
        language: Option<String>,
        source: &'static str,
    ) -> Result<(), String> {
        let text = text.trim().to_string();
        if text.is_empty() {
            return Ok(());
        }
        let subtitle = Subtitle {
            id: None,
            text,
            language,
            started_at: None,
            ended_at: None,
            source: source.into(),
            created_at: now_iso8601(),
        };
        let database = Arc::clone(&self.database);
        let history_limit = self.history_limit;
        let saved = tokio::task::spawn_blocking(move || {
            database
                .lock()
                .map_err(|_| "数据库锁不可用".to_string())?
                .add_subtitle(&subtitle, history_limit)
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| format!("字幕存储任务异常退出：{error}"))??;
        let _ = self.subtitles.send(saved);
        Ok(())
    }
}
