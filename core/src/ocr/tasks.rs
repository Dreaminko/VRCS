use super::{BlockTranslation, TextBlock, TranslatedBlock, VrOcrService};
use futures_util::{stream, StreamExt};
use serde::Serialize;
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanOutcome {
    Complete,
    NoText,
    LowConfidence,
    SourceOnly,
    PartialFailure,
    TotalFailure,
    TimedOut,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScanSummary {
    pub outcome: ScanOutcome,
    pub success_count: usize,
    pub failure_count: usize,
    pub timed_out: bool,
}

pub struct ScanResult {
    pub blocks: [Vec<TranslatedBlock>; 2],
    pub summary: ScanSummary,
}

#[derive(Clone)]
pub struct ScanConfiguration(pub(super) crate::config::AppConfig);

impl ScanConfiguration {
    pub fn ocr(&self) -> &crate::config::VrOcrConfig {
        &self.0.vr_overlay.ocr
    }
}

/// Complete block snapshots. Positions belong only to this scan and eye.
#[derive(Debug, Clone)]
pub struct BlockUpdate {
    pub scan_id: u64,
    pub eye: usize,
    pub target_language: Option<String>,
    pub block: TranslatedBlock,
}

pub fn source_view(config: &crate::config::VrOcrConfig) -> bool {
    config
        .targets
        .iter()
        .all(|target| target.profile_id.is_none())
}

impl ScanSummary {
    pub fn from_blocks(
        blocks: &[Vec<TranslatedBlock>; 2],
        source_only: bool,
        timed_out: bool,
    ) -> Self {
        let translations = blocks
            .iter()
            .flatten()
            .flat_map(|block| &block.translations);
        let mut success_count = 0;
        let mut failure_count = 0;
        for translation in translations {
            if translation
                .text
                .as_ref()
                .is_some_and(|text| !text.trim().is_empty())
            {
                success_count += 1;
            } else {
                failure_count += 1;
            }
        }
        let outcome = if source_only && blocks.iter().any(|eye| !eye.is_empty()) {
            ScanOutcome::SourceOnly
        } else if success_count > 0 && failure_count > 0 {
            ScanOutcome::PartialFailure
        } else if failure_count > 0 {
            if timed_out {
                ScanOutcome::TimedOut
            } else {
                ScanOutcome::TotalFailure
            }
        } else if success_count > 0 {
            ScanOutcome::Complete
        } else if timed_out {
            ScanOutcome::TimedOut
        } else {
            ScanOutcome::NoText
        };
        Self {
            outcome,
            success_count,
            failure_count,
            timed_out,
        }
    }
}

impl VrOcrService {
    pub fn configuration(&self) -> Result<ScanConfiguration, String> {
        self.config
            .read()
            .map(|config| ScanConfiguration(config.clone()))
            .map_err(|_| "OCR config lock failed".into())
    }

    pub fn configuration_matches(&self, snapshot: &ScanConfiguration) -> bool {
        self.matches_config(&snapshot.0)
    }

    pub(super) fn matches_config(&self, snapshot: &crate::config::AppConfig) -> bool {
        self.config.read().is_ok_and(|current| {
            current.vr_overlay.ocr == snapshot.vr_overlay.ocr
                && current.translation.prompt == snapshot.translation.prompt
                && current.asr.api_profiles == snapshot.asr.api_profiles
        })
    }

    pub(super) async fn translate_scan<F: std::future::Future<Output = Result<(), String>>>(
        &self,
        config: &crate::config::AppConfig,
        mut images: Vec<Vec<TextBlock>>,
        scan_id: u64,
        deadline: tokio::time::Instant,
        verify: impl FnOnce(super::TextRegions) -> F,
        mut progress: impl FnMut(super::Phase),
        mut completed: impl FnMut(BlockUpdate),
    ) -> Result<ScanResult, String> {
        if images.len() != 2 {
            return Err("Missing OCR eye result".into());
        }
        let had_text = images.iter().any(|eye| !eye.is_empty());
        let ocr = &config.vr_overlay.ocr;
        for eye in &mut images {
            eye.retain(|block| block.confidence >= ocr.minimum_confidence);
        }
        let regions =
            std::array::from_fn(|eye| images[eye].iter().map(|block| block.polygon).collect());
        tokio::time::timeout_at(deadline, verify(regions))
            .await
            .map_err(|_| "OCR task timed out")??;
        if !self.matches_config(config) {
            return Err("OCR configuration changed".into());
        }
        let source_only = source_view(ocr);
        let images: [Vec<TextBlock>; 2] =
            images.try_into().map_err(|_| "Missing OCR eye result")?;
        let mut blocks: [Vec<TranslatedBlock>; 2] = images.map(|eye| {
            eye.into_iter()
                .map(|source| TranslatedBlock {
                    source,
                    translations: if source_only {
                        vec![]
                    } else {
                        ocr.targets
                            .iter()
                            .map(|target| BlockTranslation {
                                target_language: target.target_language.clone(),
                                text: None,
                                error_code: Some(
                                    if target.profile_id.is_some() {
                                        "translation.timeout"
                                    } else {
                                        "translation.not_configured"
                                    }
                                    .into(),
                                ),
                            })
                            .collect()
                    },
                })
                .collect()
        });
        for (eye, eye_blocks) in blocks.iter().enumerate() {
            for block in eye_blocks {
                completed(BlockUpdate {
                    scan_id,
                    eye,
                    target_language: None,
                    block: block.clone(),
                });
            }
        }
        if source_only || blocks.iter().all(|eye| eye.is_empty()) {
            let mut summary = ScanSummary::from_blocks(&blocks, source_only, false);
            if had_text && summary.outcome == ScanOutcome::NoText {
                summary.outcome = ScanOutcome::LowConfidence;
            }
            return Ok(ScanResult { blocks, summary });
        }
        progress(super::Phase::Translating);
        let mut seen = HashSet::new();
        let texts: Vec<String> = blocks
            .iter()
            .flatten()
            .filter_map(|block| {
                seen.insert(block.source.text.clone())
                    .then(|| block.source.text.clone())
            })
            .collect();
        let requests: Vec<_> = texts
            .into_iter()
            .flat_map(|text| {
                ocr.targets
                    .iter()
                    .enumerate()
                    .filter(|(_, target)| target.profile_id.is_some())
                    .map(move |(target_index, target)| (text.clone(), target_index, target.clone()))
            })
            .collect();
        let mut pending = stream::iter(requests)
            .map(|(text, target_index, target)| async move {
                let result = self
                    .translation
                    .translate(
                        &target,
                        &config.translation.prompt,
                        &config.asr.api_profiles,
                        &text,
                        None,
                        &[],
                    )
                    .await;
                let translation = match result {
                    Ok(result) if !result.text.trim().is_empty() => BlockTranslation {
                        target_language: target.target_language.clone(),
                        text: Some(result.text),
                        error_code: None,
                    },
                    Ok(_) => BlockTranslation {
                        target_language: target.target_language.clone(),
                        text: None,
                        error_code: Some("translation.empty_result".into()),
                    },
                    Err(error) => BlockTranslation {
                        target_language: target.target_language.clone(),
                        text: None,
                        error_code: Some(error.code.into()),
                    },
                };
                (text, target_index, translation)
            })
            .buffer_unordered(4);
        let mut timed_out = false;
        loop {
            let next = tokio::select! {
                biased;
                next = pending.next() => next,
                _ = tokio::time::sleep_until(deadline) => { timed_out = true; break; }
            };
            let Some((text, target_index, translation)) = next else {
                break;
            };
            if !self.matches_config(config) {
                return Err("OCR configuration changed".into());
            }
            for (eye, eye_blocks) in blocks.iter_mut().enumerate() {
                for block in eye_blocks
                    .iter_mut()
                    .filter(|block| block.source.text == text)
                {
                    block.translations[target_index] = translation.clone();
                    completed(BlockUpdate {
                        scan_id,
                        eye,
                        target_language: Some(translation.target_language.clone()),
                        block: block.clone(),
                    });
                }
            }
        }
        drop(pending);
        let summary = ScanSummary::from_blocks(&blocks, false, timed_out);
        Ok(ScanResult { blocks, summary })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::post, Json, Router};
    use serde_json::json;
    use std::sync::{Arc, RwLock};
    use std::time::Duration;

    fn block(id: usize, text: &str) -> TextBlock {
        TextBlock {
            id,
            text: text.into(),
            confidence: 0.95,
            polygon: [[10., 10.], [80., 10.], [80., 30.], [10., 30.]],
        }
    }

    fn service(origin: Option<String>) -> (VrOcrService, crate::config::AppConfig) {
        let mut config = crate::config::AppConfig::default();
        if let Some(origin) = origin {
            config.asr.api_profiles.push(crate::config::ApiProfile {
                id: "ocr-test".into(),
                provider: crate::providers::OPENAI_COMPATIBLE_PROVIDER.into(),
                base_url: Some(origin),
                auth_mode: crate::config::ApiAuthMode::None,
                is_local: true,
                enabled_capabilities: vec![crate::providers::CAPABILITY_TEXT_TRANSLATION.into()],
                ..crate::config::ApiProfile::default()
            });
            config.vr_overlay.ocr.targets[0].profile_id = Some("ocr-test".into());
        }
        let service = VrOcrService::new(
            Arc::new(RwLock::new(config.clone())),
            Arc::new(crate::translation::TranslationService::new().unwrap()),
            Arc::new(super::super::LocalOcrRuntime::new(
                std::env::temp_dir().join("unused-ocr-task-models"),
            )),
        )
        .unwrap();
        (service, config)
    }

    #[tokio::test]
    async fn ocr_source_only_no_text_and_low_confidence_have_distinct_outcomes() {
        let (service, config) = service(None);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        let source = service
            .translate_scan(
                &config,
                vec![vec![block(7, "source")], vec![]],
                17,
                deadline,
                |_| async { Ok(()) },
                |_| panic!("Source viewing must not start translation"),
                |update| {
                    assert_eq!(
                        (update.scan_id, update.eye, update.block.source.id),
                        (17, 0, 7)
                    );
                    assert!(update.block.translations.is_empty());
                },
            )
            .await
            .unwrap();
        assert_eq!(source.summary.outcome, ScanOutcome::SourceOnly);
        let empty = service
            .translate_scan(
                &config,
                vec![vec![], vec![]],
                18,
                deadline,
                |_| async { Ok(()) },
                |_| {},
                |_| {},
            )
            .await
            .unwrap();
        assert_eq!(empty.summary.outcome, ScanOutcome::NoText);
        let mut low = block(0, "low");
        low.confidence = 0.1;
        let low = service
            .translate_scan(
                &config,
                vec![vec![low], vec![]],
                19,
                deadline,
                |_| async { Ok(()) },
                |_| {},
                |_| {},
            )
            .await
            .unwrap();
        assert_eq!(low.summary.outcome, ScanOutcome::LowConfidence);
    }

    #[tokio::test]
    async fn ocr_completed_translations_survive_other_requests_timing_out() {
        let release = Arc::new(tokio::sync::Notify::new());
        let handler_release = release.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route(
                    "/chat/completions",
                    post(move |Json(body): Json<serde_json::Value>| {
                        let release = handler_release.clone();
                        async move {
                            if body["messages"].to_string().contains("slow") {
                                release.notified().await;
                            }
                            Json(json!({"choices":[{"message":{"content":"done"}}]}))
                        }
                    }),
                ),
            )
            .await
            .unwrap();
        });
        let (service, config) = service(Some(origin));
        let mut updates = Vec::new();
        let result = service
            .translate_scan(
                &config,
                vec![
                    vec![block(10, "fast"), block(11, "slow")],
                    vec![block(20, "fast")],
                ],
                42,
                tokio::time::Instant::now() + Duration::from_millis(300),
                |_| async { Ok(()) },
                |_| {},
                |update| updates.push(update),
            )
            .await
            .unwrap();
        release.notify_waiters();
        server.abort();
        assert_eq!(result.summary.outcome, ScanOutcome::PartialFailure);
        assert!(result.summary.timed_out);
        assert_eq!(
            (result.summary.success_count, result.summary.failure_count),
            (2, 1)
        );
        assert_eq!(
            result.blocks[0][0].translations[0].text.as_deref(),
            Some("done")
        );
        assert_eq!(
            result.blocks[0][1].translations[0].error_code.as_deref(),
            Some("translation.timeout")
        );
        let completed: Vec<_> = updates
            .iter()
            .filter(|update| update.target_language.is_some())
            .collect();
        assert_eq!(completed.len(), 2);
        assert_eq!(
            (
                completed[1].scan_id,
                completed[1].eye,
                completed[1].block.source.id
            ),
            (42, 1, 20)
        );
    }

    #[tokio::test]
    async fn ocr_total_failure_is_not_recognition_success() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route(
                    "/chat/completions",
                    post(|| async {
                        (
                            axum::http::StatusCode::BAD_REQUEST,
                            Json(json!({"error":{"message":"rejected"}})),
                        )
                    }),
                ),
            )
            .await
            .unwrap();
        });
        let (service, config) = service(Some(origin));
        let result = service
            .translate_scan(
                &config,
                vec![vec![block(0, "text")], vec![]],
                2,
                tokio::time::Instant::now() + Duration::from_secs(2),
                |_| async { Ok(()) },
                |_| {},
                |_| {},
            )
            .await
            .unwrap();
        server.abort();
        assert_eq!(result.summary.outcome, ScanOutcome::TotalFailure);
        assert_eq!(
            (result.summary.success_count, result.summary.failure_count),
            (0, 1)
        );
        assert!(!result.summary.timed_out);
    }

    #[tokio::test]
    async fn ocr_configuration_replacement_rejects_in_flight_completions() {
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let handler_started = started.clone();
        let handler_release = release.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route(
                    "/chat/completions",
                    post(move || {
                        let started = handler_started.clone();
                        let release = handler_release.clone();
                        async move {
                            started.notify_one();
                            release.notified().await;
                            Json(json!({"choices":[{"message":{"content":"old settings"}}]}))
                        }
                    }),
                ),
            )
            .await
            .unwrap();
        });
        let (service, config) = service(Some(origin));
        let current = service.config.clone();
        let work = tokio::spawn(async move {
            service
                .translate_scan(
                    &config,
                    vec![vec![block(0, "text")], vec![]],
                    3,
                    tokio::time::Instant::now() + Duration::from_secs(2),
                    |_| async { Ok(()) },
                    |_| {},
                    |update| {
                        assert!(
                            update.target_language.is_none(),
                            "Changed settings must not emit a completion"
                        );
                    },
                )
                .await
        });
        tokio::time::timeout(Duration::from_secs(1), started.notified())
            .await
            .unwrap();
        current.write().unwrap().vr_overlay.ocr.minimum_confidence = 0.9;
        release.notify_one();
        let error = match work.await.unwrap() {
            Ok(_) => panic!("Old settings must be rejected"),
            Err(error) => error,
        };
        server.abort();
        assert_eq!(error, "OCR configuration changed");
    }
}
