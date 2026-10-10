use super::{BlockTranslation, TextBlock, TranslatedBlock, VrOcrService};
use futures_util::{stream, stream::FuturesUnordered, Stream, StreamExt};
use serde::Serialize;
use std::collections::{HashMap, VecDeque};

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
        &self.0.ocr
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

#[derive(Debug, Clone, Copy)]
pub(super) enum TranslationProgress {
    ImageDone(usize),
    InputDone,
    Registered,
    Finished(bool),
}

#[derive(Clone)]
struct TranslationRequest {
    key: [u8; 32],
    text: String,
    context: Vec<crate::translation::TranslationContextEntry>,
    target_index: usize,
    target: crate::config::TranslationTargetConfig,
    subscribers: Vec<(usize, usize)>,
    translation: Option<BlockTranslation>,
}

fn group_context(
    groups: &[super::layout::TextGroup],
    index: usize,
    prompt: &crate::config::TranslationPromptConfig,
) -> Vec<crate::translation::TranslationContextEntry> {
    let mut nearby: Vec<_> = groups
        .iter()
        .enumerate()
        .filter(|(other, group)| *other != index && group.region == groups[index].region)
        .collect();
    nearby.sort_by_key(|(other, _)| other.abs_diff(index));
    let mut remaining = prompt.max_chars.saturating_sub(256) as usize;
    let mut selected = Vec::new();
    for (other, group) in nearby {
        if selected.len() >= prompt.max_messages as usize {
            break;
        }
        let cost = serde_json::to_string(&group.source.text)
            .unwrap()
            .chars()
            .count()
            + 16;
        if cost <= remaining {
            remaining -= cost;
            selected.push((
                other,
                crate::translation::TranslationContextEntry {
                    source: "OCR".into(),
                    text: group.source.text.clone(),
                    created_at: String::new(),
                },
            ));
        }
    }
    selected.sort_by_key(|(other, _)| *other);
    selected.into_iter().map(|(_, context)| context).collect()
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
        } else if success_count > 0 && (failure_count > 0 || timed_out) {
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
            .map(|config| ScanConfiguration(crate::config::apply_feature_gates(&config)))
            .map_err(|_| "OCR config lock failed".into())
    }

    pub fn configuration_matches(&self, snapshot: &ScanConfiguration) -> bool {
        self.matches_config(&snapshot.0)
    }

    pub(super) fn matches_config(&self, snapshot: &crate::config::AppConfig) -> bool {
        self.config.read().is_ok_and(|current| {
            let current = crate::config::apply_feature_gates(&current);
            current.ocr == snapshot.ocr
                && current.translation.prompt == snapshot.translation.prompt
                && current.asr.api_profiles == snapshot.asr.api_profiles
        })
    }

    // Match the public scan boundary without adding a wrapper for its three callbacks.
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn translate_scan<F: std::future::Future<Output = Result<(), String>>>(
        &self,
        config: &crate::config::AppConfig,
        mut images: Vec<Vec<TextBlock>>,
        scan_id: u64,
        deadline: tokio::time::Instant,
        verify: impl FnOnce(super::TextRegions) -> F,
        progress: impl FnMut(super::Phase),
        completed: impl FnMut(BlockUpdate),
    ) -> Result<ScanResult, String> {
        if !(1..=2).contains(&images.len()) {
            return Err("OCR requires one or two images".into());
        }
        images.resize_with(2, Vec::new);
        let regions = std::array::from_fn(|eye| {
            images[eye]
                .iter()
                .filter(|block| block.confidence >= config.ocr.minimum_confidence)
                .map(|block| block.polygon)
                .collect()
        });
        tokio::time::timeout_at(deadline, verify(regions))
            .await
            .map_err(|_| "OCR task timed out")??;
        self.translate_stream(
            config,
            stream::iter(images.into_iter().enumerate().map(Ok)),
            scan_id,
            deadline,
            progress,
            completed,
        )
        .await
    }

    pub(super) async fn translate_stream(
        &self,
        config: &crate::config::AppConfig,
        images: impl Stream<Item = Result<(usize, Vec<TextBlock>), String>>,
        scan_id: u64,
        deadline: tokio::time::Instant,
        progress: impl FnMut(super::Phase),
        completed: impl FnMut(BlockUpdate),
    ) -> Result<ScanResult, String> {
        self.translate_stream_with_progress(
            config,
            images,
            scan_id,
            deadline,
            progress,
            completed,
            |_| {},
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn translate_stream_with_progress(
        &self,
        config: &crate::config::AppConfig,
        images: impl Stream<Item = Result<(usize, Vec<TextBlock>), String>>,
        scan_id: u64,
        deadline: tokio::time::Instant,
        mut progress: impl FnMut(super::Phase),
        mut completed: impl FnMut(BlockUpdate),
        mut pipeline: impl FnMut(TranslationProgress),
    ) -> Result<ScanResult, String> {
        use sha2::{Digest, Sha256};
        let started = std::time::Instant::now();
        let ocr = &config.ocr;
        let source_only = source_view(ocr);
        let glossary = self
            .translation
            .ocr_glossary_fingerprint(&config.translation.prompt);
        let mut blocks: [Vec<TranslatedBlock>; 2] = Default::default();
        let mut requests: Vec<TranslationRequest> = Vec::new();
        let mut seen = HashMap::new();
        let mut queued: VecDeque<usize> = VecDeque::new();
        let mut pending = FuturesUnordered::new();
        let mut images_done = false;
        let mut had_text = false;
        let mut translating = false;
        let mut timed_out = false;
        let mut cache_hits = 0usize;
        let mut first_translation_ms = None;
        let mut config_check = tokio::time::interval(std::time::Duration::from_millis(100));
        futures_util::pin_mut!(images);
        loop {
            if !self.matches_config(config) {
                return Err("OCR configuration changed".into());
            }
            while pending.len() < 4 {
                let Some(index) = queued.pop_front() else {
                    break;
                };
                let request = requests[index].clone();
                pending.push(async move {
                    let (translation, cached) =
                        self.translate_request(config, glossary, request).await;
                    (index, translation, cached)
                });
            }
            if images_done && pending.is_empty() {
                break;
            }
            tokio::select! {
                biased;
                Some((request_index, translation, cached)) = pending.next(), if !pending.is_empty() => {
                    if !self.matches_config(config) {
                        return Err("OCR configuration changed".into());
                    }
                    cache_hits += usize::from(cached);
                    if first_translation_ms.is_none() && translation.text.is_some() {
                        first_translation_ms = Some(started.elapsed().as_millis() as u64);
                    }
                    let request = &mut requests[request_index];
                    request.translation = Some(translation.clone());
                    pipeline(TranslationProgress::Finished(translation.text.as_ref().is_none_or(|text| text.trim().is_empty())));
                    for &(eye, index) in &request.subscribers {
                        let block = &mut blocks[eye][index];
                        block.translations[request.target_index] = translation.clone();
                        completed(BlockUpdate {
                            scan_id, eye, target_language: Some(translation.target_language.clone()),
                            block: block.clone(),
                        });
                    }
                }
                _ = tokio::time::sleep_until(deadline) => {
                    timed_out = true;
                    break;
                }
                next = images.next(), if !images_done => {
                    let Some(next) = next else {
                        images_done = true;
                        pipeline(TranslationProgress::InputDone);
                        continue;
                    };
                    let (eye, image) = next?;
                    if eye >= 2 { return Err("Invalid OCR eye result".into()); }
                    had_text |= !image.is_empty();
                    let groups = super::layout::group_blocks(image, ocr.minimum_confidence);
                    let contexts: Vec<_> = (0..groups.len())
                        .map(|index| group_context(&groups, index, &config.translation.prompt)).collect();
                    blocks[eye] = groups.into_iter().map(|group| TranslatedBlock {
                        source: group.source,
                        fragments: group.fragments,
                        translations: if source_only { vec![] } else {
                            ocr.targets.iter().map(|target| BlockTranslation {
                                target_language: target.target_language.clone(),
                                text: None,
                                error_code: Some(if target.profile_id.is_some() {
                                    "translation.timeout"
                                } else { "translation.not_configured" }.into()),
                            }).collect()
                        },
                    }).collect();
                    for (index, block) in blocks[eye].iter_mut().enumerate() {
                        completed(BlockUpdate { scan_id, eye, target_language: None, block: block.clone() });
                        if source_only { continue; }
                        for (target_index, target) in ocr.targets.iter().enumerate()
                            .filter(|(_, target)| target.profile_id.is_some())
                        {
                            if !translating { progress(super::Phase::Translating); translating = true; }
                            let context = &contexts[index];
                            let context_key: Vec<_> = context.iter()
                                .map(|entry| (&entry.source, &entry.text)).collect();
                            let profile = config.asr.api_profiles.iter()
                                .find(|profile| Some(&profile.id) == target.profile_id.as_ref());
                            let key: [u8; 32] = Sha256::digest(serde_json::to_vec(&(
                                &block.source.text, target, profile, &config.translation.prompt,
                                context_key, glossary,
                            )).expect("OCR translation cache key is serializable")).into();
                            // Pending and completed requests share subscribers; target slots stay separate.
                            if let Some(&request_index) = seen.get(&(key, target_index)) {
                                let request: &mut TranslationRequest = &mut requests[request_index];
                                request.subscribers.push((eye, index));
                                if let Some(translation) = &request.translation {
                                    block.translations[target_index] = translation.clone();
                                    completed(BlockUpdate {
                                        scan_id, eye, target_language: Some(translation.target_language.clone()),
                                        block: block.clone(),
                                    });
                                }
                            } else {
                                seen.insert((key, target_index), requests.len());
                                queued.push_back(requests.len());
                                requests.push(TranslationRequest {
                                    key, text: block.source.text.clone(), context: context.clone(),
                                    target_index, target: target.clone(),
                                    subscribers: vec![(eye, index)], translation: None,
                                });
                                pipeline(TranslationProgress::Registered);
                            }
                        }
                    }
                    pipeline(TranslationProgress::ImageDone(eye));
                }
                _ = config_check.tick() => {}
            }
        }
        drop(pending);
        let mut summary = ScanSummary::from_blocks(&blocks, source_only, timed_out);
        if had_text && summary.outcome == ScanOutcome::NoText {
            summary.outcome = ScanOutcome::LowConfidence;
        }
        tracing::debug!(
            scan_id,
            elapsed_ms = started.elapsed().as_millis() as u64,
            cache_hits,
            ?first_translation_ms,
            requests = requests.len(),
            blocks = blocks.iter().map(Vec::len).sum::<usize>(),
            timed_out,
            "OCR translations processed"
        );
        Ok(ScanResult { blocks, summary })
    }

    async fn translate_request(
        &self,
        config: &crate::config::AppConfig,
        glossary: [u8; 32],
        request: TranslationRequest,
    ) -> (BlockTranslation, bool) {
        let cached = self
            .cache
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(&request.key);
        if let Some(cached) = cached {
            return (
                BlockTranslation {
                    target_language: request.target.target_language,
                    text: Some(cached),
                    error_code: None,
                },
                true,
            );
        }
        let result = self
            .translation
            .translate(
                &request.target,
                &config.translation.prompt,
                &config.asr.api_profiles,
                &request.text,
                None,
                &request.context,
            )
            .await;
        let translation = match result {
            Ok(result) if !result.text.trim().is_empty() => BlockTranslation {
                target_language: request.target.target_language,
                text: Some(result.text),
                error_code: None,
            },
            Ok(_) => BlockTranslation {
                target_language: request.target.target_language,
                text: None,
                error_code: Some("translation.empty_result".into()),
            },
            Err(error) => BlockTranslation {
                target_language: request.target.target_language,
                text: None,
                error_code: Some(error.code.into()),
            },
        };
        if self.matches_config(config)
            && glossary
                == self
                    .translation
                    .ocr_glossary_fingerprint(&config.translation.prompt)
        {
            if let Some(text) = &translation.text {
                self.cache
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .insert(request.key, text.clone());
            }
        }
        (translation, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::post, Json, Router};
    use serde_json::json;
    use std::sync::{Arc, RwLock};
    use std::time::Duration;

    #[tokio::test]
    async fn progress_counts_target_slots_and_cached_requests_without_double_counting_eyes() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = calls.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route(
                    "/chat/completions",
                    post(move || {
                        let counted = counted.clone();
                        async move {
                            counted.fetch_add(1, Ordering::SeqCst);
                            Json(json!({"choices":[{"message":{"content":"translated"}}]}))
                        }
                    }),
                ),
            )
            .await
            .unwrap();
        });
        let (service, mut config) = service(Some(origin));
        config.ocr.targets.push(config.ocr.targets[0].clone());
        *service.config.write().unwrap() = config.clone();
        for scan_id in [1, 2] {
            let mut snapshots = Vec::new();
            let mut reporter = super::super::progress::Reporter::new(scan_id, 2, |snapshot| {
                snapshots.push(snapshot)
            });
            let result = service
                .translate_stream_with_progress(
                    &config,
                    stream::iter([
                        Ok((0, vec![block(0, "same")])),
                        Ok((1, vec![block(0, "same")])),
                    ]),
                    scan_id,
                    tokio::time::Instant::now() + Duration::from_secs(2),
                    |_| {},
                    |_| {},
                    |event| reporter.translation(event),
                )
                .await
                .unwrap();
            let last = snapshots.last().unwrap();
            assert_eq!(
                (
                    last.translation_total,
                    last.translation_completed,
                    last.translation_failed
                ),
                (2, 2, 0)
            );
            assert!(last.translation_total_final && last.recognition_done);
            assert_eq!(last.images_done, 2);
            assert_eq!(result.summary.success_count, 4);
            assert_eq!(
                calls.load(Ordering::SeqCst),
                2,
                "The second scan must use cached translations"
            );
        }
        server.abort();
    }

    fn block(id: usize, text: &str) -> TextBlock {
        TextBlock {
            id,
            text: text.into(),
            confidence: 0.95,
            polygon: [[10., 10.], [80., 10.], [80., 30.], [10., 30.]],
        }
    }

    fn positioned(id: usize, text: &str, x: f32, y: f32, width: f32) -> TextBlock {
        TextBlock {
            polygon: [[x, y], [x + width, y], [x + width, y + 20.], [x, y + 20.]],
            ..block(id, text)
        }
    }

    async fn layout_scan(images: Vec<Vec<TextBlock>>) -> ScanResult {
        let (service, config) = service(None);
        service
            .translate_scan(
                &config,
                images,
                1,
                tokio::time::Instant::now() + Duration::from_secs(2),
                |_| async { Ok(()) },
                |_| {},
                |_| {},
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn desktop_layout_joins_fragments_and_wrapped_sentences_before_translation() {
        let result = layout_scan(vec![vec![
            positioned(2, "where this is?", 10., 36., 170.),
            positioned(1, "tell me", 125., 12., 125.),
            positioned(0, "Could you", 10., 10., 110.),
        ]])
        .await;
        assert_eq!(result.blocks[0].len(), 1);
        assert_eq!(
            result.blocks[0][0].source.text,
            "Could you tell me where this is?"
        );
        assert_eq!(
            result.blocks[0][0].source.polygon,
            [[10., 10.], [250., 10.], [250., 56.], [10., 56.]]
        );
        let result = layout_scan(vec![vec![
            positioned(0, "この文章は", 10., 10., 190.),
            positioned(1, "次の行に続きます。", 10., 34., 180.),
        ]])
        .await;
        assert_eq!(
            result.blocks[0][0].source.text,
            "この文章は次の行に続きます。"
        );
    }

    #[tokio::test]
    async fn desktop_layout_keeps_other_columns_labels_and_finished_sentences_separate() {
        for texts in [
            ("This sentence ends.", "another sentence"),
            ("Alice", "hello from another player"),
            ("Open Settings", "Open Folder"),
        ] {
            let result = layout_scan(vec![vec![
                positioned(0, texts.0, 10., 10., 220.),
                positioned(1, texts.1, 10., 34., 180.),
            ]])
            .await;
            assert_eq!(result.blocks[0].len(), 2, "{texts:?}");
        }
        let separate = vec![
            positioned(0, "Could you tell me", 10., 10., 220.),
            positioned(1, "text from another column", 400., 10., 220.),
            positioned(2, "text much further down", 10., 120., 180.),
        ];
        assert_eq!(layout_scan(vec![separate.clone()]).await.blocks[0].len(), 3);
        let vr = vec![
            positioned(0, "Could you tell me", 10., 10., 220.),
            positioned(1, "where this is?", 10., 34., 180.),
        ];
        assert_eq!(layout_scan(vec![vr, vec![]]).await.blocks[0].len(), 1);
    }

    #[tokio::test]
    async fn desktop_layout_keeps_adjacent_buttons_and_speakers_separate() {
        for (left, right) in [
            ("Cancel", "Save"),
            ("Alice:", "hello there"),
            ("取消", "保存"),
        ] {
            let result = layout_scan(vec![vec![
                positioned(0, left, 10., 10., 70.),
                positioned(1, right, 90., 10., 50.),
            ]])
            .await;
            assert_eq!(result.blocks[0].len(), 2, "{left} / {right}");
        }
    }

    #[tokio::test]
    async fn vr_layout_joins_short_and_capitalized_continuations() {
        for (first, second, expected) in [
            ("I think", "I know the way.", "I think I know the way."),
            (
                "Meet me at",
                "Central Station.",
                "Meet me at Central Station.",
            ),
            ("どこに", "行きますか？", "どこに行きますか？"),
        ] {
            let result = layout_scan(vec![
                vec![
                    positioned(9, second, 10., 34., 180.),
                    positioned(4, first, 10., 10., 150.),
                ],
                vec![],
            ])
            .await;
            assert_eq!(result.blocks[0].len(), 1, "{expected}");
            assert_eq!(result.blocks[0][0].source.text, expected);
        }
    }

    #[tokio::test]
    async fn layout_keeps_a_multisentence_paragraph_together() {
        let result = layout_scan(vec![vec![
            positioned(0, "We arrived at the station.", 10., 10., 250.),
            positioned(1, "The next train leaves soon.", 10., 34., 250.),
        ]])
        .await;
        assert_eq!(result.blocks[0].len(), 1);
        assert_eq!(
            result.blocks[0][0].source.text,
            "We arrived at the station. The next train leaves soon."
        );
    }

    #[tokio::test]
    async fn layout_keeps_low_confidence_text_as_a_merge_barrier() {
        let mut barrier = positioned(1, "another speaker", 10., 25., 150.);
        barrier.confidence = 0.01;
        let result = layout_scan(vec![vec![
            positioned(0, "Please tell me", 10., 10., 180.),
            barrier,
            positioned(2, "where to go", 10., 34., 180.),
        ]])
        .await;
        assert_eq!(result.blocks[0].len(), 2);
    }

    #[tokio::test]
    async fn single_image_keeps_sources_without_creating_a_second_image() {
        let (service, config) = service(None);
        let mut updates = Vec::new();
        let result = service
            .translate_scan(
                &config,
                vec![vec![block(0, "hello")]],
                7,
                tokio::time::Instant::now() + Duration::from_secs(2),
                |regions| async move {
                    assert_eq!(regions[0].len(), 1);
                    assert!(regions[1].is_empty());
                    Ok(())
                },
                |_| {},
                |update| updates.push(update),
            )
            .await
            .unwrap();
        assert_eq!(result.blocks[0][0].source.text, "hello");
        assert!(result.blocks[1].is_empty());
        assert_eq!(result.summary.outcome, ScanOutcome::SourceOnly);
        assert_eq!(updates.len(), 1);
        assert_eq!((updates[0].scan_id, updates[0].eye), (7, 0));
    }

    #[tokio::test]
    async fn streamed_eyes_translate_before_the_next_eye_and_share_in_flight_requests() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let requests = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let (count, notify, wait) = (requests.clone(), started.clone(), release.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route(
                    "/chat/completions",
                    post(move || {
                        let (count, notify, wait) = (count.clone(), notify.clone(), wait.clone());
                        async move {
                            count.fetch_add(1, Ordering::SeqCst);
                            notify.notify_one();
                            wait.notified().await;
                            Json(json!({"choices":[{"message":{"content":"translated"}}]}))
                        }
                    }),
                ),
            )
            .await
            .unwrap();
        });
        let (service, config) = service(Some(origin));
        let eyes =
            stream::once(async { Ok((0, vec![block(0, "same")])) }).chain(stream::once(async {
                started.notified().await;
                Ok((1, vec![positioned(0, "same", 100., 10., 70.)]))
            }));
        let mut updates = Vec::new();
        let mut progress = Vec::new();
        let result = service
            .translate_stream_with_progress(
                &config,
                eyes,
                81,
                tokio::time::Instant::now() + Duration::from_secs(2),
                |_| {},
                |update| {
                    if update.eye == 1 && update.target_language.is_none() {
                        release.notify_one();
                    }
                    updates.push(update);
                },
                |event| progress.push(event),
            )
            .await
            .unwrap();
        server.abort();
        assert!(
            !result.summary.timed_out,
            "Eye 1 can finish only after translation starts"
        );
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        assert_eq!(
            progress
                .iter()
                .filter(|event| matches!(event, TranslationProgress::Registered))
                .count(),
            1
        );
        assert_eq!(
            progress
                .iter()
                .filter(|event| matches!(event, TranslationProgress::Finished(false)))
                .count(),
            1
        );
        assert!(
            progress
                .iter()
                .position(|event| matches!(event, TranslationProgress::ImageDone(0)))
                .unwrap()
                < progress
                    .iter()
                    .position(|event| matches!(event, TranslationProgress::ImageDone(1)))
                    .unwrap()
        );
        assert_eq!(
            progress
                .iter()
                .filter(|event| matches!(event, TranslationProgress::InputDone))
                .count(),
            1
        );
        assert!(
            progress
                .iter()
                .position(|event| matches!(event, TranslationProgress::InputDone))
                .unwrap()
                > progress
                    .iter()
                    .position(|event| matches!(event, TranslationProgress::ImageDone(1)))
                    .unwrap()
        );
        assert_eq!(result.summary.success_count, 2);
        assert_eq!(result.blocks[1][0].source.polygon[0][0], 100.);
        let translated: Vec<_> = updates
            .iter()
            .filter(|update| update.target_language.is_some())
            .collect();
        assert_eq!(
            translated
                .iter()
                .map(|update| (update.scan_id, update.eye))
                .collect::<Vec<_>>(),
            [(81, 0), (81, 1)]
        );
    }

    #[tokio::test]
    async fn streamed_deadline_keeps_completed_first_eye_while_recognition_is_pending() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route(
                    "/chat/completions",
                    post(|| async { Json(json!({"choices":[{"message":{"content":"done"}}]})) }),
                ),
            )
            .await
            .unwrap();
        });
        let (service, config) = service(Some(origin));
        let eyes =
            stream::once(async { Ok((0, vec![block(0, "first")])) }).chain(stream::pending());
        let result = service
            .translate_stream(
                &config,
                eyes,
                1,
                tokio::time::Instant::now() + Duration::from_millis(200),
                |_| {},
                |_| {},
            )
            .await
            .unwrap();
        server.abort();
        assert!(result.summary.timed_out);
        assert_eq!(result.summary.outcome, ScanOutcome::PartialFailure);
        assert_eq!(
            result.blocks[0][0].translations[0].text.as_deref(),
            Some("done")
        );
        assert!(result.blocks[1].is_empty());
    }

    #[tokio::test]
    async fn streamed_translation_drop_stops_queued_requests_after_four_active_calls() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let requests = Arc::new(AtomicUsize::new(0));
        let active = requests.clone();
        let release = Arc::new(tokio::sync::Notify::new());
        let wait = release.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route(
                    "/chat/completions",
                    post(move || {
                        let (active, wait) = (active.clone(), wait.clone());
                        async move {
                            active.fetch_add(1, Ordering::SeqCst);
                            wait.notified().await;
                            Json(json!({"choices":[{"message":{"content":"done"}}]}))
                        }
                    }),
                ),
            )
            .await
            .unwrap();
        });
        let (service, config) = service(Some(origin));
        let eyes = stream::iter([
            Ok((
                0,
                (0..4).map(|id| block(id, &format!("left {id}"))).collect(),
            )),
            Ok((
                1,
                (0..4).map(|id| block(id, &format!("right {id}"))).collect(),
            )),
        ]);
        let mut scan = Box::pin(service.translate_stream(
            &config,
            eyes,
            1,
            tokio::time::Instant::now() + Duration::from_secs(2),
            |_| {},
            |_| {},
        ));
        tokio::select! {
            result = &mut scan => panic!("Scan finished unexpectedly: {}", result.is_ok()),
            _ = async {
                while requests.load(Ordering::SeqCst) < 4 {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            } => {}
            _ = tokio::time::sleep(Duration::from_secs(1)) => panic!("No four active requests"),
        }
        assert_eq!(requests.load(Ordering::SeqCst), 4);
        drop(scan);
        release.notify_waiters();
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(requests.load(Ordering::SeqCst), 4);
        server.abort();
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
            config.ocr.targets[0].profile_id = Some("ocr-test".into());
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
    async fn ocr_translation_reuses_cache_without_unrelated_scan_context() {
        let bodies = Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded = bodies.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route(
                    "/chat/completions",
                    post(move |Json(body): Json<serde_json::Value>| {
                        let recorded = recorded.clone();
                        async move {
                            recorded.lock().unwrap().push(body);
                            Json(json!({"choices":[{"message":{"content":"translated"}}]}))
                        }
                    }),
                ),
            )
            .await
            .unwrap();
        });
        let (service, mut config) = service(Some(origin));
        for other in ["context", "context", "different"] {
            let result = service
                .translate_scan(
                    &config,
                    vec![
                        vec![block(0, "hello"), block(1, other)],
                        vec![block(0, "hello")],
                    ],
                    1,
                    tokio::time::Instant::now() + Duration::from_secs(2),
                    |_| async { Ok(()) },
                    |_| {},
                    |_| {},
                )
                .await
                .unwrap();
            assert_eq!(result.summary.outcome, ScanOutcome::Complete);
            let count = bodies.lock().unwrap().len();
            assert_eq!(count, if other == "context" { 2 } else { 3 });
        }
        let bodies_snapshot = bodies.lock().unwrap().clone();
        let hello = bodies_snapshot
            .iter()
            .find(|body| body["messages"].to_string().contains("hello"))
            .unwrap();
        assert!(!hello["messages"].to_string().contains("REFERENCE CONTEXT"));
        assert!(!hello["messages"].to_string().contains("context"));
        config.ocr.targets[0].model = "different-model".into();
        *service.config.write().unwrap() = config.clone();
        service
            .translate_scan(
                &config,
                vec![vec![block(0, "hello"), block(1, "different")], vec![]],
                2,
                tokio::time::Instant::now() + Duration::from_secs(2),
                |_| async { Ok(()) },
                |_| {},
                |_| {},
            )
            .await
            .unwrap();
        assert_eq!(bodies.lock().unwrap().len(), 5);
        config
            .translation
            .prompt
            .system_prompt
            .push_str(" Keep UI labels brief.");
        *service.config.write().unwrap() = config.clone();
        service
            .translate_scan(
                &config,
                vec![vec![block(0, "hello"), block(1, "different")], vec![]],
                3,
                tokio::time::Instant::now() + Duration::from_secs(2),
                |_| async { Ok(()) },
                |_| {},
                |_| {},
            )
            .await
            .unwrap();
        assert_eq!(bodies.lock().unwrap().len(), 7);
        config.asr.api_profiles[0]
            .headers
            .push(crate::config::HttpHeaderConfig {
                name: "x-cache-test".into(),
                value: "changed".into(),
            });
        *service.config.write().unwrap() = config.clone();
        service
            .translate_scan(
                &config,
                vec![vec![block(0, "hello"), block(1, "different")], vec![]],
                4,
                tokio::time::Instant::now() + Duration::from_secs(2),
                |_| async { Ok(()) },
                |_| {},
                |_| {},
            )
            .await
            .unwrap();
        assert_eq!(bodies.lock().unwrap().len(), 9);
        server.abort();
    }

    #[tokio::test]
    async fn translation_request_contains_assembled_source_and_no_unrelated_context() {
        let bodies = Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded = bodies.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route(
                    "/chat/completions",
                    post(move |Json(body): Json<serde_json::Value>| {
                        let recorded = recorded.clone();
                        async move {
                            recorded.lock().unwrap().push(body);
                            Json(json!({"choices":[{"message":{"content":"translated"}}]}))
                        }
                    }),
                ),
            )
            .await
            .unwrap();
        });
        let (service, config) = service(Some(origin));
        let fragments = vec![
            positioned(8, "Could you", 10., 10., 110.),
            positioned(3, "tell me", 125., 10., 125.),
            positioned(6, "where this is?", 10., 34., 180.),
        ];
        let mut inputs = fragments.clone();
        inputs.push(positioned(1, "Settings", 400., 10., 150.));
        let result = service
            .translate_scan(
                &config,
                vec![inputs],
                9,
                tokio::time::Instant::now() + Duration::from_secs(2),
                |_| async { Ok(()) },
                |_| {},
                |_| {},
            )
            .await
            .unwrap();
        assert_eq!(result.blocks[0][0].fragments, fragments);
        let bodies = bodies.lock().unwrap();
        assert_eq!(bodies.len(), 2);
        let assembled = bodies
            .iter()
            .find(|body| {
                body["messages"]
                    .to_string()
                    .contains("Could you tell me where this is?")
            })
            .expect("one complete source in the LLM request");
        let messages = assembled["messages"].to_string();
        assert!(!messages.contains("Settings"));
        assert!(!messages.contains("REFERENCE CONTEXT"));
        server.abort();
    }

    #[tokio::test]
    async fn paragraph_chunks_translate_with_local_context_and_distinct_subscribers() {
        let bodies = Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded = bodies.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route(
                    "/chat/completions",
                    post(move |Json(body): Json<serde_json::Value>| {
                        let recorded = recorded.clone();
                        async move {
                            let messages = body["messages"].to_string();
                            let translation = if messages.contains('甲') {
                                "first region"
                            } else {
                                "second region"
                            };
                            recorded.lock().unwrap().push(body);
                            Json(json!({"choices":[{"message":{"content":translation}}]}))
                        }
                    }),
                ),
            )
            .await
            .unwrap();
        });
        let (service, config) = service(Some(origin));
        let common = "同じ文章".repeat(275);
        let mut updates = Vec::new();
        for _ in 0..2 {
            let eye = vec![
                positioned(0, &common, 10., 10., 180.),
                positioned(1, &"甲の説明".repeat(275), 10., 34., 180.),
                positioned(2, &common, 400., 10., 180.),
                positioned(3, &"乙の説明".repeat(275), 400., 34., 180.),
            ];
            let result = service
                .translate_scan(
                    &config,
                    vec![eye.clone(), eye],
                    11,
                    tokio::time::Instant::now() + Duration::from_secs(2),
                    |_| async { Ok(()) },
                    |_| {},
                    |update| updates.push(update),
                )
                .await
                .unwrap();
            assert_eq!(result.blocks[0].len(), 4);
            assert_eq!(result.summary.success_count, 8);
            assert_eq!(
                result.blocks[0][0].translations[0].text.as_deref(),
                Some("first region")
            );
            assert_eq!(
                result.blocks[0][2].translations[0].text.as_deref(),
                Some("second region")
            );
        }
        let bodies = bodies.lock().unwrap();
        assert_eq!(
            bodies.len(),
            4,
            "identical stereo inputs and repeated scans reuse translations"
        );
        for body in bodies.iter() {
            let messages = body["messages"].to_string();
            assert!(messages.contains("REFERENCE CONTEXT"));
            assert!(!(messages.contains('甲') && messages.contains('乙')));
        }
        assert_eq!(
            updates
                .iter()
                .filter(|update| update.target_language.is_some())
                .count(),
            16
        );
        server.abort();
    }

    #[tokio::test]
    async fn ocr_translation_does_not_cache_errors_or_empty_results() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let count = Arc::new(AtomicUsize::new(0));
        let requested = count.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, Router::new().route("/chat/completions", post(move || {
                let count = requested.clone();
                async move {
                    let index = count.fetch_add(1, Ordering::SeqCst);
                    if index == 0 { (axum::http::StatusCode::BAD_REQUEST, Json(json!({"error":{"message":"rejected"}}))) }
                    else { (axum::http::StatusCode::OK, Json(json!({"choices":[{"message":{"content":if index == 1 {""} else {"translated"}}}]}))) }
                }
            }))).await.unwrap();
        });
        let (service, config) = service(Some(origin));
        for expected in [
            ScanOutcome::TotalFailure,
            ScanOutcome::TotalFailure,
            ScanOutcome::Complete,
            ScanOutcome::Complete,
        ] {
            let result = service
                .translate_scan(
                    &config,
                    vec![vec![block(0, "text")], vec![]],
                    1,
                    tokio::time::Instant::now() + Duration::from_secs(2),
                    |_| async { Ok(()) },
                    |_| {},
                    |_| {},
                )
                .await
                .unwrap();
            assert_eq!(result.summary.outcome, expected);
        }
        assert_eq!(count.load(Ordering::SeqCst), 3);
        server.abort();
    }

    #[tokio::test]
    async fn ocr_glossary_changes_during_requests_do_not_cache_obsolete_translations() {
        use crate::config::{GlossaryConfig, GlossaryEntry, GlossarySource};
        use std::sync::atomic::{AtomicUsize, Ordering};
        fn glossary(term: &str) -> GlossaryConfig {
            GlossaryConfig {
                sources: vec![GlossarySource::Local {
                    id: "terms".into(),
                    name: "terms".into(),
                    enabled: true,
                    entries: vec![GlossaryEntry {
                        source: term.into(),
                        target: Some("term".into()),
                        category: Default::default(),
                        case_sensitive: false,
                    }],
                }],
                ..Default::default()
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(
            crate::glossary::GlossaryStore::new(
                directory.path().join("glossary.json"),
                glossary("Alpha"),
            )
            .unwrap(),
        );
        let count = Arc::new(AtomicUsize::new(0));
        let requested = count.clone();
        let handler_store = store.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route(
                    "/chat/completions",
                    post(move || {
                        let count = requested.clone();
                        let store = handler_store.clone();
                        async move {
                            if count.fetch_add(1, Ordering::SeqCst) == 0 {
                                store.set_config(glossary("Beta"));
                            }
                            Json(json!({"choices":[{"message":{"content":"translated"}}]}))
                        }
                    }),
                ),
            )
            .await
            .unwrap();
        });
        let (mut service, config) = service(Some(origin));
        service.translation =
            Arc::new(crate::translation::TranslationService::with_glossary(store.clone()).unwrap());
        for (index, expected) in [1, 2, 3, 3, 4].into_iter().enumerate() {
            if index == 2 {
                store.set_config(glossary("Alpha"));
            }
            if index == 4 {
                store.set_config(glossary("Gamma"));
            }
            service
                .translate_scan(
                    &config,
                    vec![vec![block(0, "text")], vec![]],
                    index as u64,
                    tokio::time::Instant::now() + Duration::from_secs(2),
                    |_| async { Ok(()) },
                    |_| {},
                    |_| {},
                )
                .await
                .unwrap();
            assert_eq!(count.load(Ordering::SeqCst), expected);
        }
        server.abort();
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
                        (17, 0, 0)
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
                            if body["messages"].as_array().unwrap().last().unwrap()["content"]
                                .as_str()
                                .unwrap()
                                .ends_with("\n\nslow")
                            {
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
            (42, 1, 0)
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
        current.write().unwrap().ocr.minimum_confidence = 0.9;
        release.notify_one();
        let error = match work.await.unwrap() {
            Ok(_) => panic!("Old settings must be rejected"),
            Err(error) => error,
        };
        server.abort();
        assert_eq!(error, "OCR configuration changed");
    }
}
