use crate::config::{ApiProfile, AsrConfig};
use crate::providers::{self, RecognitionTransport, ServiceAdapter};

use super::openai_audio_transcriptions;
use super::read_credential;
use super::segmented_upload::SegmentedUploadSession;
use super::streaming::{self, CloudEvent, SegmentationMode, StreamingSession};

pub enum CloudRecognitionSession {
    Realtime(StreamingSession),
    Segmented(SegmentedUploadSession),
}

impl CloudRecognitionSession {
    pub async fn send(&self, samples: Vec<f32>) -> Result<(), String> {
        match self {
            Self::Realtime(session) => session.send(samples).await,
            Self::Segmented(session) => session.send(samples).await,
        }
    }

    pub async fn commit(&self, valid_segment: bool) -> Result<Option<String>, String> {
        match self {
            Self::Realtime(session) => session.commit().await.map(|()| None),
            Self::Segmented(session) if valid_segment => session.commit().await.map(Some),
            Self::Segmented(session) => {
                session.discard_pending().await;
                Ok(None)
            }
        }
    }

    pub fn segmentation_mode(&self) -> SegmentationMode {
        match self {
            Self::Realtime(session) => session.segmentation_mode(),
            Self::Segmented(session) => session.segmentation_mode(),
        }
    }

    pub async fn recv(&mut self) -> Option<CloudEvent> {
        match self {
            Self::Realtime(session) => session.recv().await,
            Self::Segmented(session) => session.recv().await,
        }
    }

    pub async fn stop(self) {
        match self {
            Self::Realtime(session) => session.stop().await,
            Self::Segmented(session) => session.stop().await,
        }
    }

    pub async fn stop_and_drain(self) -> Vec<CloudEvent> {
        match self {
            Self::Realtime(session) => session.stop_and_drain().await,
            Self::Segmented(session) => session.stop_and_drain().await,
        }
    }
}

pub fn validate_cloud_connection(config: &AsrConfig) -> Result<(), String> {
    let (profile, service) = active_profile_service(config)?;
    match service.recognition_transport {
        Some(RecognitionTransport::RealtimeStream) => streaming::validate_cloud_connection(config),
        Some(RecognitionTransport::SegmentedUpload) => {
            if service.adapter != ServiceAdapter::OpenAiAudioTranscriptions {
                return Err(format!(
                    "Unsupported segmented recognition adapter: {:?}",
                    service.adapter
                ));
            }
            credential(profile)?;
            let settings = service_settings(config, service.id)?;
            if settings.model.trim().is_empty() {
                return Err(format!("A model is required for service {}", service.id));
            }
            providers::effective_base_url(profile).map(|_| ())
        }
        None => Err(format!(
            "Service {} is not a cloud recognition service",
            service.id
        )),
    }
}

pub async fn spawn_cloud_recognition_session(
    config: AsrConfig,
    silence_seconds: f64,
) -> Result<CloudRecognitionSession, String> {
    let (profile, service) = active_profile_service(&config)?;
    match service.recognition_transport {
        Some(RecognitionTransport::RealtimeStream) => {
            streaming::spawn_streaming_session(config, silence_seconds)
                .await
                .map(CloudRecognitionSession::Realtime)
        }
        Some(RecognitionTransport::SegmentedUpload) => {
            if service.adapter != ServiceAdapter::OpenAiAudioTranscriptions {
                return Err(format!(
                    "Unsupported segmented recognition adapter: {:?}",
                    service.adapter
                ));
            }
            let profile = profile.clone();
            let key = credential(&profile)?;
            let settings = service_settings(&config, service.id)?.clone();
            if settings.model.trim().is_empty() {
                return Err(format!("A model is required for service {}", service.id));
            }
            Ok(CloudRecognitionSession::Segmented(
                SegmentedUploadSession::spawn(profile, key, settings, config.language),
            ))
        }
        None => Err(format!(
            "Service {} is not a cloud recognition service",
            service.id
        )),
    }
}

pub async fn test_cloud_service(
    config: &AsrConfig,
    profile_id: &str,
    service_id: &str,
) -> Result<(), String> {
    let profile = config
        .api_profiles
        .iter()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(|| "API profile does not exist".to_string())?;
    let resolved = providers::resolve_profile_service(profile, service_id)?;
    match resolved.service.recognition_transport {
        Some(RecognitionTransport::RealtimeStream) => {
            streaming::test_streaming_connection(config, profile_id, Some(service_id)).await
        }
        Some(RecognitionTransport::SegmentedUpload) => {
            if resolved.service.adapter != ServiceAdapter::OpenAiAudioTranscriptions {
                return Err(format!(
                    "Unsupported segmented recognition adapter: {:?}",
                    resolved.service.adapter
                ));
            }
            let key = credential(profile)?;
            let settings = service_settings(config, service_id)?;
            let samples = connection_test_audio();
            openai_audio_transcriptions::test_connection(
                &reqwest::Client::new(),
                profile,
                &key,
                settings,
                &config.language,
                &samples,
            )
            .await
        }
        None => Err(format!(
            "Service {service_id} is not a cloud recognition service"
        )),
    }
}

fn active_profile_service(
    config: &AsrConfig,
) -> Result<(&ApiProfile, providers::ProviderServiceDefinition), String> {
    let active_id = config
        .active_profile_id
        .as_deref()
        .ok_or_else(|| format!("No API profile is selected for service {}", config.backend))?;
    let profile = config
        .api_profiles
        .iter()
        .find(|profile| profile.id == active_id)
        .ok_or_else(|| "The active API profile does not exist".to_string())?;
    let resolved = providers::resolve_profile_service(profile, &config.backend)?;
    Ok((profile, *resolved.service))
}

fn service_settings<'a>(
    config: &'a AsrConfig,
    service_id: &str,
) -> Result<&'a crate::config::RecognitionServiceSettings, String> {
    config
        .service_settings
        .get(service_id)
        .ok_or_else(|| format!("Recognition settings are missing for service {service_id}"))
}

fn credential(profile: &ApiProfile) -> Result<String, String> {
    if !profile.requires_api_key() {
        return Ok(String::new());
    }
    read_credential(&profile.id, &profile.provider)?
        .ok_or_else(|| format!("API key is not configured for {}", profile.name))
}

fn connection_test_audio() -> Vec<f32> {
    let sample_count = 16_000 / 4;
    (0..sample_count)
        .map(|index| {
            let phase = index as f32 * 440.0 * std::f32::consts::TAU / 16_000.0;
            phase.sin() * 0.01
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_test_audio_is_non_empty_and_bounded() {
        let samples = connection_test_audio();
        assert_eq!(samples.len(), 4_000);
        assert!(samples.iter().any(|sample| *sample != 0.0));
        assert!(samples.iter().all(|sample| sample.abs() <= 0.01));
    }
}
