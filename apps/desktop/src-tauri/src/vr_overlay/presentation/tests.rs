// These assertions cover rendered text; source metadata has dedicated tests below.
impl PresentationContent {
    fn without_original(mut self) -> Self {
        let (Self::Headset(messages) | Self::Wrist(messages)) = &mut self;
        for message in messages {
            message.original = None;
        }
        self
    }
}

use super::*;

fn headset_content(text: String) -> PresentationContent {
    PresentationFrame::headset(text, 1.0)
        .content
        .without_original()
}

#[test]
fn preview_switches_select_original_and_translation_in_previews_and_finals() {
    let now = Instant::now();
    for (show_original, show_translation, expected_preview, expected_final) in [
        (true, true, "Hello.\n你好。", "Hello.\n已完成的译文"),
        (true, false, "Hello.", "Hello."),
        (false, true, "你好。", "已完成的译文"),
    ] {
        let headset_config = VrOverlayHeadsetConfig {
            show_partials: show_original,
            show_translation_partials: show_translation,
            ..Default::default()
        };
        let wrist_config = VrOverlayWristConfig {
            show_partials: show_original,
            show_translation_partials: show_translation,
            ..Default::default()
        };
        let mut headset = HeadsetPresentation::default();
        let mut wrist = WristPresentation::default();
        let preview = |text: &str| PresentationEvent::LiveTranslationUpdated {
            source: "speaker".into(),
            snapshot: vrcs_core::LiveTranslation {
                utterance_id: "preview-1".into(),
                source_utterance_id: Some("source-1".into()),
                completed_original: None,
                conversation_preview: None,
                speaker: None,
                text: text.into(),
                language: Some("en".into()),
                translation: "你好。".into(),
                target_language: "zh".into(),
            },
        };
        let assert_text =
            |headset: &HeadsetPresentation, wrist: &WristPresentation, expected: &str| {
                assert_eq!(
                    headset
                        .frame(now, &headset_config)
                        .unwrap()
                        .content
                        .without_original(),
                    headset_content(expected.into()),
                    "original={show_original}, translation={show_translation}",
                );
                assert_eq!(
                    wrist
                        .frame(now, &wrist_config)
                        .unwrap()
                        .content
                        .without_original(),
                    PresentationContent::Wrist(vec![WristMessage {
                        original: None,
                        speaker: None,
                        text: expected.into(),
                        side: MessageSide::Left,
                    }]),
                    "original={show_original}, translation={show_translation}",
                );
            };
        let event = preview("Hello.");
        headset.apply(event.clone(), now, &headset_config);
        wrist.apply(event, now, &wrist_config);
        assert_text(&headset, &wrist, expected_preview);

        let mut completed = subtitle(1, "Hello.");
        completed.translations.push(translation("已完成的译文"));
        for event in [
            PresentationEvent::Final {
                utterance_id: Some("source-1".into()),
                subtitle: completed,
            },
            preview(""),
        ] {
            headset.apply(event.clone(), now, &headset_config);
            wrist.apply(event, now, &wrist_config);
        }
        assert_text(&headset, &wrist, expected_preview);

        let event = PresentationEvent::RecognitionCancelled {
            source: "speaker".into(),
            utterance_id: "preview-1".into(),
        };
        headset.apply(event.clone(), now, &headset_config);
        wrist.apply(event, now, &wrist_config);
        assert_text(&headset, &wrist, expected_final);
    }
}

#[test]
fn delayed_native_previews_keep_both_lanes_after_display_history_is_evicted() {
    let now = Instant::now();
    for newer_count in [3, 32] {
        for (show_original, show_translation, expected) in [
            (true, true, "Older original.\n迟到译文"),
            (true, false, "Older original."),
            (false, true, "迟到译文"),
        ] {
            let headset_config = VrOverlayHeadsetConfig {
                show_partials: show_original,
                show_translation_partials: show_translation,
                ..Default::default()
            };
            let wrist_config = VrOverlayWristConfig {
                show_partials: show_original,
                show_translation_partials: show_translation,
                max_entries: 3,
                ..Default::default()
            };
            let mut headset = HeadsetPresentation::default();
            let mut wrist = WristPresentation::default();
            for index in 0..=newer_count {
                let event = PresentationEvent::Final {
                    utterance_id: Some(format!("qwen-source-{index}")),
                    subtitle: subtitle(
                        index + 1,
                        if index == 0 {
                            "Older original."
                        } else {
                            "Newer original."
                        },
                    ),
                };
                headset.apply(event.clone(), now, &headset_config);
                wrist.apply(event, now, &wrist_config);
            }
            let event = PresentationEvent::LiveTranslationUpdated {
                source: "speaker".into(),
                snapshot: vrcs_core::LiveTranslation {
                    utterance_id: "qwen-preview-0".into(),
                    source_utterance_id: Some("qwen-source-0".into()),
                    completed_original: Some("Older original.".into()),
                    conversation_preview: None,
                    speaker: None,
                    text: String::new(),
                    language: Some("en".into()),
                    translation: "迟到译文".into(),
                    target_language: "zh-Hans".into(),
                },
            };
            headset.apply(event.clone(), now, &headset_config);
            wrist.apply(event, now, &wrist_config);
            assert_eq!(
                headset
                    .frame(now, &headset_config)
                    .unwrap()
                    .content
                    .without_original(),
                headset_content(expected.into())
            );
            let PresentationContent::Wrist(messages) = wrist
                .frame(now, &wrist_config)
                .unwrap()
                .content
                .without_original()
            else {
                panic!()
            };
            assert_eq!(messages.last().unwrap().text, expected);
            assert!(messages.len() <= 3);
            let event = PresentationEvent::RecognitionCancelled {
                source: "speaker".into(),
                utterance_id: "qwen-preview-0".into(),
            };
            headset.apply(event.clone(), now, &headset_config);
            wrist.apply(event, now, &wrist_config);
            assert!(headset.partials.is_empty() && wrist.partials.is_empty());
        }
    }
}

fn subtitle(id: i64, text: &str) -> Subtitle {
    Subtitle {
        speaker: None,
        id: Some(id),
        conversation_id: None,
        text: text.into(),
        language: Some("en".into()),
        started_at: None,
        ended_at: None,
        source: "speaker".into(),
        created_at: "2026-01-01T00:00:00Z".into(),
        translations: Vec::new(),
    }
}

fn final_event(subtitle: Subtitle) -> PresentationEvent {
    PresentationEvent::Final {
        utterance_id: None,
        subtitle,
    }
}

fn partial_event(utterance_id: &str, source: &str, text: &str) -> PresentationEvent {
    PresentationEvent::RecognitionPartial {
        utterance_id: utterance_id.into(),
        source: source.into(),
        text: text.into(),
        language: Some("en".into()),
        speaker: None,
    }
}

fn translation(text: &str) -> vrcs_core::SubtitleTranslation {
    translation_in("zh", text)
}

#[test]
fn wrist_without_visible_content_has_no_frame() {
    let now = Instant::now();
    for idle_hide_seconds in [0, 15] {
        let mut config = VrOverlayWristConfig {
            idle_hide_seconds,
            ..Default::default()
        };
        let mut wrist = WristPresentation::default();
        assert!(wrist.frame(now, &config).is_none());
        wrist.apply(final_event(subtitle(1, " \n ")), now, &config);
        assert!(wrist.frame(now, &config).is_none());
        wrist.apply(final_event(subtitle(2, "hello")), now, &config);
        config.include_speaker = false;
        assert!(wrist.frame(now, &config).is_none());
    }
}

#[test]
fn wrist_idle_deadline_preserves_history_and_new_activity_restores_visibility() {
    let now = Instant::now();
    let config = VrOverlayWristConfig {
        idle_hide_seconds: 15,
        ..Default::default()
    };
    let mut wrist = WristPresentation::default();
    wrist.apply(final_event(subtitle(1, "hello")), now, &config);
    assert!(wrist
        .frame(now + Duration::from_millis(14999), &config)
        .is_some());
    assert!(wrist
        .frame(now + Duration::from_secs(15), &config)
        .is_none());
    assert!(wrist
        .frame(now + Duration::from_secs(60), &config)
        .is_none());
    assert_eq!(wrist.entries.len(), 1);

    wrist.apply(
        final_event(subtitle(2, "again")),
        now + Duration::from_secs(60),
        &config,
    );
    let frame = wrist.frame(now + Duration::from_secs(60), &config).unwrap();
    assert_eq!(
        frame.content.without_original(),
        PresentationContent::Wrist(vec![
            WristMessage {
                original: None,
                speaker: None,
                text: "hello".into(),
                side: MessageSide::Left
            },
            WristMessage {
                original: None,
                speaker: None,
                text: "again".into(),
                side: MessageSide::Left
            },
        ])
    );
    assert!(wrist
        .frame(now + Duration::from_secs(75), &config)
        .is_none());
}

#[test]
fn wrist_partials_and_late_translation_restart_idle_deadline() {
    let now = Instant::now();
    let config = VrOverlayWristConfig {
        idle_hide_seconds: 15,
        show_partials: true,
        ..Default::default()
    };
    let mut wrist = WristPresentation::default();
    wrist.apply(final_event(subtitle(1, "hello")), now, &config);
    wrist.apply(
        partial_event("u2", "speaker", "again"),
        now + Duration::from_secs(10),
        &config,
    );
    assert!(wrist
        .frame(now + Duration::from_secs(15), &config)
        .is_some());
    assert!(wrist
        .frame(now + Duration::from_secs(25), &config)
        .is_none());
    wrist.apply(
        PresentationEvent::TranslationCompleted {
            subtitle_id: 1,
            translation: translation("你好"),
            preferred: true,
        },
        now + Duration::from_secs(30),
        &config,
    );
    assert!(wrist
        .frame(now + Duration::from_secs(30), &config)
        .is_some());
    assert!(wrist
        .frame(now + Duration::from_secs(45), &config)
        .is_none());
}

#[test]
fn wrist_ignored_events_do_not_extend_idle_deadline() {
    let now = Instant::now();
    let config = VrOverlayWristConfig {
        idle_hide_seconds: 15,
        show_partials: false,
        show_translation_partials: false,
        ..Default::default()
    };
    let mut excluded = subtitle(2, "local");
    excluded.source = "microphone".into();
    let events = [
        final_event(subtitle(2, " \n ")),
        final_event(excluded),
        partial_event("u2", "speaker", "disabled preview"),
        PresentationEvent::TranslationPartial {
            subtitle_id: 1,
            text: "disabled translation".into(),
            target_language: "zh".into(),
            preferred: true,
        },
        PresentationEvent::TranslationCompleted {
            subtitle_id: 99,
            translation: translation("unmatched"),
            preferred: true,
        },
        PresentationEvent::TranslationCompleted {
            subtitle_id: 1,
            translation: translation("  "),
            preferred: true,
        },
        PresentationEvent::RecognitionCancelled {
            utterance_id: "u2".into(),
            source: "speaker".into(),
        },
        PresentationEvent::RecognitionReset {
            source: "speaker".into(),
        },
    ];
    for event in events {
        let mut wrist = WristPresentation::default();
        wrist.apply(final_event(subtitle(1, "hello")), now, &config);
        wrist.apply(event.clone(), now + Duration::from_secs(10), &config);
        assert!(
            wrist
                .frame(now + Duration::from_secs(15), &config)
                .is_none(),
            "{event:?}"
        );
    }
}

#[test]
fn wrist_empty_live_preview_does_not_extend_idle_deadline() {
    let now = Instant::now();
    let config = VrOverlayWristConfig {
        idle_hide_seconds: 15,
        show_partials: true,
        show_translation_partials: false,
        ..Default::default()
    };
    let mut wrist = WristPresentation::default();
    wrist.apply(final_event(subtitle(1, "hello")), now, &config);
    wrist.apply(
        PresentationEvent::LiveTranslationUpdated {
            source: "speaker".into(),
            snapshot: vrcs_core::LiveTranslation {
                utterance_id: "u2".into(),
                source_utterance_id: None,
                completed_original: None,
                conversation_preview: None,
                speaker: None,
                text: "  ".into(),
                language: Some("en".into()),
                translation: "hidden translation".into(),
                target_language: "zh".into(),
            },
        },
        now + Duration::from_secs(10),
        &config,
    );
    assert!(wrist
        .frame(now + Duration::from_secs(15), &config)
        .is_none());
}

#[test]
fn wrist_translation_from_disabled_source_does_not_extend_idle_deadline() {
    let now = Instant::now();
    for partial in [false, true] {
        let mut config = VrOverlayWristConfig {
            idle_hide_seconds: 15,
            include_microphone: true,
            show_translation_partials: partial,
            ..Default::default()
        };
        let mut wrist = WristPresentation::default();
        let mut local = subtitle(2, "local");
        local.source = "microphone".into();
        wrist.apply(final_event(local), now, &config);
        wrist.apply(final_event(subtitle(1, "hello")), now, &config);
        config.include_microphone = false;
        let event = if partial {
            PresentationEvent::TranslationPartial {
                subtitle_id: 2,
                text: "本地".into(),
                target_language: "zh".into(),
                preferred: true,
            }
        } else {
            PresentationEvent::TranslationCompleted {
                subtitle_id: 2,
                translation: translation("本地"),
                preferred: true,
            }
        };
        wrist.apply(event, now + Duration::from_secs(10), &config);
        assert!(wrist
            .frame(now + Duration::from_secs(15), &config)
            .is_none());
    }
}

#[test]
fn wrist_zero_timeout_only_disables_idle_hiding() {
    let now = Instant::now();
    let mut config = VrOverlayWristConfig {
        idle_hide_seconds: 0,
        show_partials: true,
        ..Default::default()
    };
    let mut wrist = WristPresentation::default();
    wrist.apply(partial_event("u1", "speaker", "hello"), now, &config);
    assert!(wrist
        .frame(now + Duration::from_secs(3600), &config)
        .is_some());
    wrist.apply(
        PresentationEvent::RecognitionCancelled {
            utterance_id: "u1".into(),
            source: "speaker".into(),
        },
        now,
        &config,
    );
    assert!(wrist.frame(now, &config).is_none());
    wrist.apply(final_event(subtitle(1, "hello")), now, &config);
    assert!(wrist
        .frame(now + Duration::from_secs(3600), &config)
        .is_some());
    config.idle_hide_seconds = 15;
    assert!(wrist
        .frame(now + Duration::from_secs(3600), &config)
        .is_none());
}

fn translation_in(target_language: &str, text: &str) -> vrcs_core::SubtitleTranslation {
    vrcs_core::SubtitleTranslation {
        source_group: None,
        text: text.into(),
        source_language: Some("en".into()),
        target_language: target_language.into(),
        provider: "test".into(),
        model: None,
        created_at: "2026-01-01T00:00:00Z".into(),
    }
}

#[test]
fn headset_final_fades() {
    let now = Instant::now();
    let config = VrOverlayHeadsetConfig {
        display_seconds: 2.0,
        fade_seconds: 1.0,
        ..Default::default()
    };
    let mut state = HeadsetPresentation::default();
    state.apply(final_event(subtitle(7, "hello")), now, &config);
    let fading = state
        .frame(now + Duration::from_millis(2500), &config)
        .unwrap();
    assert_eq!(
        fading.content.without_original(),
        headset_content("hello".into())
    );
    assert!((fading.opacity - 0.5).abs() < 0.01);
    assert!(state.frame(now + Duration::from_secs(3), &config).is_none());
}

#[test]
fn translation_completion_updates_matching_headset_item() {
    let now = Instant::now();
    let config = VrOverlayHeadsetConfig {
        ..Default::default()
    };
    let mut state = HeadsetPresentation::default();
    state.apply(final_event(subtitle(7, "hello")), now, &config);
    state.apply(
        PresentationEvent::TranslationCompleted {
            subtitle_id: 7,
            translation: translation("你好"),
            preferred: true,
        },
        now + Duration::from_secs(1),
        &config,
    );
    assert_eq!(
        state
            .frame(now, &config)
            .unwrap()
            .content
            .without_original(),
        headset_content("hello\n你好".into())
    );
}

#[test]
fn multilingual_translation_display_can_show_preferred_or_all_languages() {
    let now = Instant::now();
    let headset_config = VrOverlayHeadsetConfig {
        ..Default::default()
    };
    let wrist_config = VrOverlayWristConfig {
        show_translation_partials: true,
        ..Default::default()
    };
    let events = [
        PresentationEvent::TranslationCompleted {
            subtitle_id: 7,
            translation: translation_in("zh", "你好"),
            preferred: true,
        },
        PresentationEvent::TranslationCompleted {
            subtitle_id: 7,
            translation: translation_in("ja", "こんにちは"),
            preferred: false,
        },
    ];
    let mut headset = HeadsetPresentation::default();
    let mut wrist = WristPresentation::default();
    headset.apply(final_event(subtitle(7, "hello")), now, &headset_config);
    wrist.apply(final_event(subtitle(7, "hello")), now, &wrist_config);
    for event in events {
        headset.apply(event.clone(), now, &headset_config);
        wrist.apply(event, now, &wrist_config);
    }

    assert_eq!(
        headset
            .frame_with_translation_display(now, &headset_config, "preferred_only")
            .unwrap()
            .content
            .without_original(),
        headset_content("hello\n你好".into())
    );
    assert_eq!(
        headset
            .frame_with_translation_display(now, &headset_config, "all_languages")
            .unwrap()
            .content
            .without_original(),
        headset_content("hello\n你好\nこんにちは".into())
    );
    assert_eq!(
        wrist
            .frame_with_translation_display(now, &wrist_config, "preferred_only")
            .unwrap()
            .content
            .without_original(),
        PresentationContent::Wrist(vec![WristMessage {
            original: None,
            speaker: None,
            text: "hello\n你好".into(),
            side: MessageSide::Left,
        }])
    );
    assert_eq!(
        wrist
            .frame_with_translation_display(now, &wrist_config, "all_languages")
            .unwrap()
            .content
            .without_original(),
        PresentationContent::Wrist(vec![WristMessage {
            original: None,
            speaker: None,
            text: "hello\n你好\nこんにちは".into(),
            side: MessageSide::Left,
        }])
    );
}

#[test]
fn interleaved_wrist_translation_partials_keep_each_language_current() {
    let now = Instant::now();
    let config = VrOverlayWristConfig {
        show_translation_partials: true,
        idle_hide_seconds: 3,
        ..Default::default()
    };
    let mut wrist = WristPresentation::default();
    wrist.apply(final_event(subtitle(7, "hello")), now, &config);
    for (target_language, text, preferred, elapsed) in [
        ("zh", "你", true, 1),
        ("ja", "こん", false, 2),
        ("zh", "你好", true, 3),
        ("ja", "こんにちは", false, 4),
    ] {
        wrist.apply(
            PresentationEvent::TranslationPartial {
                subtitle_id: 7,
                text: text.into(),
                target_language: target_language.into(),
                preferred,
            },
            now + Duration::from_secs(elapsed),
            &config,
        );
    }

    assert_eq!(
        wrist
            .frame_with_translation_display(now + Duration::from_secs(6), &config, "all_languages",)
            .unwrap()
            .content
            .without_original(),
        PresentationContent::Wrist(vec![WristMessage {
            original: None,
            speaker: None,
            text: "hello\n你好\nこんにちは".into(),
            side: MessageSide::Left,
        }])
    );
}

#[test]
fn later_non_preferred_update_preserves_preferred_translation() {
    let now = Instant::now();
    let config = VrOverlayHeadsetConfig {
        show_translation_partials: true,
        ..Default::default()
    };
    let mut headset = HeadsetPresentation::default();
    headset.apply(final_event(subtitle(7, "hello")), now, &config);
    for (text, preferred) in [("你", true), ("你好", false)] {
        headset.apply(
            PresentationEvent::TranslationCompleted {
                subtitle_id: 7,
                translation: translation(text),
                preferred,
            },
            now,
            &config,
        );
    }

    assert_eq!(
        headset
            .frame_with_translation_display(now, &config, "preferred_only")
            .unwrap()
            .content
            .without_original(),
        headset_content("hello\n你好".into())
    );
}

#[test]
fn same_language_translation_is_hidden() {
    let now = Instant::now();
    let mut translation = translation("hello");
    translation.source_language = None;
    translation.target_language = "en".into();

    let headset_config = VrOverlayHeadsetConfig {
        ..Default::default()
    };
    let mut headset = HeadsetPresentation::default();
    headset.apply(final_event(subtitle(7, "hello")), now, &headset_config);
    headset.apply(
        PresentationEvent::TranslationCompleted {
            subtitle_id: 7,
            translation: translation.clone(),
            preferred: true,
        },
        now,
        &headset_config,
    );
    assert_eq!(
        headset
            .frame(now, &headset_config)
            .unwrap()
            .content
            .without_original(),
        headset_content("hello".into())
    );

    let wrist_config = VrOverlayWristConfig {
        ..Default::default()
    };
    let mut item = subtitle(7, "hello");
    item.translations.push(translation);
    let mut wrist = WristPresentation::default();
    wrist.apply(final_event(item), now, &wrist_config);
    assert_eq!(
        wrist
            .frame(now, &wrist_config)
            .unwrap()
            .content
            .without_original(),
        PresentationContent::Wrist(vec![WristMessage {
            original: None,
            speaker: None,
            text: "hello".into(),
            side: MessageSide::Left,
        }])
    );
}

#[test]
fn wrist_keeps_only_configured_recent_finals() {
    let now = Instant::now();
    let config = VrOverlayWristConfig {
        max_entries: 3,
        ..Default::default()
    };
    let mut state = WristPresentation::default();
    for id in 1..=5 {
        state.apply(
            final_event(subtitle(id, &format!("line{id}"))),
            now,
            &config,
        );
    }
    assert_eq!(
        state
            .frame(now, &config)
            .unwrap()
            .content
            .without_original(),
        PresentationContent::Wrist(vec![
            WristMessage {
                original: None,
                speaker: None,
                text: "line3".into(),
                side: MessageSide::Left,
            },
            WristMessage {
                original: None,
                speaker: None,
                text: "line4".into(),
                side: MessageSide::Left,
            },
            WristMessage {
                original: None,
                speaker: None,
                text: "line5".into(),
                side: MessageSide::Left,
            },
        ])
    );
}

#[test]
fn wrist_trims_existing_history_when_limit_changes() {
    let now = Instant::now();
    let mut config = VrOverlayWristConfig {
        max_entries: 5,
        ..Default::default()
    };
    let mut state = WristPresentation::default();
    for id in 1..=5 {
        state.apply(
            final_event(subtitle(id, &format!("line{id}"))),
            now,
            &config,
        );
    }

    config.max_entries = 3;
    state.set_max_entries(config.max_entries);

    let PresentationContent::Wrist(messages) = state
        .frame(now, &config)
        .unwrap()
        .content
        .without_original()
    else {
        panic!("expected wrist messages");
    };
    assert_eq!(
        messages
            .iter()
            .map(|message| message.text.as_str())
            .collect::<Vec<_>>(),
        vec!["line3", "line4", "line5"]
    );
}

#[test]
fn wrist_places_remote_left_and_local_sources_right() {
    let now = Instant::now();
    let config = VrOverlayWristConfig {
        include_microphone: true,
        include_chatbox: true,
        ..Default::default()
    };
    let mut state = WristPresentation::default();

    for (id, source) in [(1, "speaker"), (2, "microphone"), (3, "chatbox")] {
        let mut item = subtitle(id, source);
        item.source = source.into();
        state.apply(final_event(item), now, &config);
    }

    let PresentationContent::Wrist(messages) = state
        .frame(now, &config)
        .unwrap()
        .content
        .without_original()
    else {
        panic!("expected wrist messages");
    };
    assert_eq!(messages[0].side, MessageSide::Left);
    assert_eq!(messages[1].side, MessageSide::Right);
    assert_eq!(messages[2].side, MessageSide::Right);
}

#[test]
fn recognition_partials_are_visible_only_when_enabled() {
    let now = Instant::now();
    let mut headset_config = VrOverlayHeadsetConfig {
        show_partials: false,
        ..Default::default()
    };
    let mut headset = HeadsetPresentation::default();
    headset.apply(partial_event("u1", "speaker", "hel"), now, &headset_config);
    assert!(headset.frame(now, &headset_config).is_none());

    headset_config.show_partials = true;
    headset.apply(
        partial_event("u1", "speaker", "hello"),
        now,
        &headset_config,
    );
    assert_eq!(
        headset
            .frame(now, &headset_config)
            .unwrap()
            .content
            .without_original(),
        headset_content("hello".into())
    );

    let wrist_config = VrOverlayWristConfig {
        show_partials: true,
        ..Default::default()
    };
    let mut wrist = WristPresentation::default();
    wrist.apply(partial_event("u1", "speaker", "hello"), now, &wrist_config);
    assert_eq!(
        wrist
            .frame(now, &wrist_config)
            .unwrap()
            .content
            .without_original(),
        PresentationContent::Wrist(vec![WristMessage {
            original: None,
            speaker: None,
            text: "hello".into(),
            side: MessageSide::Left,
        }])
    );
}

#[test]
fn headset_final_replaces_partial_and_blocks_late_updates() {
    let now = Instant::now();
    let config = VrOverlayHeadsetConfig {
        show_partials: true,
        ..Default::default()
    };
    let mut state = HeadsetPresentation::default();
    state.apply(partial_event("u1", "speaker", "partial"), now, &config);
    state.apply(
        PresentationEvent::Final {
            utterance_id: Some("u1".into()),
            subtitle: subtitle(1, "final"),
        },
        now,
        &config,
    );
    state.apply(partial_event("u1", "speaker", "late partial"), now, &config);

    assert_eq!(
        state
            .frame(now, &config)
            .unwrap()
            .content
            .without_original(),
        headset_content("final".into())
    );
}

#[test]
fn disabling_partials_discards_hidden_state() {
    let now = Instant::now();
    let mut config = VrOverlayWristConfig {
        show_partials: true,
        ..Default::default()
    };
    let mut state = WristPresentation::default();
    state.apply(partial_event("u1", "speaker", "partial"), now, &config);

    state.set_show_partials(false);
    config.show_partials = true;

    assert!(state.frame(now, &config).is_none());
}

#[test]
fn final_replaces_matching_wrist_partial_without_clearing_newer_utterance() {
    let now = Instant::now();
    let config = VrOverlayWristConfig {
        show_partials: true,
        include_microphone: true,
        ..Default::default()
    };
    let mut state = WristPresentation::default();
    state.apply(partial_event("u1", "speaker", "old partial"), now, &config);
    state.apply(
        partial_event("u2", "microphone", "new partial"),
        now,
        &config,
    );

    let mut final_subtitle = subtitle(1, "old final");
    final_subtitle.source = "speaker".into();
    state.apply(
        PresentationEvent::Final {
            utterance_id: Some("u1".into()),
            subtitle: final_subtitle,
        },
        now,
        &config,
    );
    state.apply(partial_event("u1", "speaker", "late partial"), now, &config);

    let PresentationContent::Wrist(messages) = state
        .frame(now, &config)
        .unwrap()
        .content
        .without_original()
    else {
        panic!("expected wrist messages");
    };
    assert_eq!(
        messages
            .iter()
            .map(|message| message.text.as_str())
            .collect::<Vec<_>>(),
        vec!["old final", "new partial"]
    );
}

#[test]
fn terminated_wrist_partial_cannot_reappear() {
    let now = Instant::now();
    let config = VrOverlayWristConfig {
        show_partials: true,
        ..Default::default()
    };
    let mut state = WristPresentation::default();
    state.apply(partial_event("u1", "speaker", "partial"), now, &config);
    state.apply(
        PresentationEvent::RecognitionCancelled {
            utterance_id: "u1".into(),
            source: "speaker".into(),
        },
        now,
        &config,
    );
    state.apply(partial_event("u1", "speaker", "late partial"), now, &config);
    assert!(state.frame(now, &config).is_none());
}

#[test]
fn recognition_reset_allows_reused_utterance_ids() {
    let now = Instant::now();
    let config = VrOverlayWristConfig {
        show_partials: true,
        ..Default::default()
    };
    let mut state = WristPresentation::default();
    state.apply(
        PresentationEvent::RecognitionCancelled {
            utterance_id: "u1".into(),
            source: "speaker".into(),
        },
        now,
        &config,
    );
    state.apply(
        PresentationEvent::RecognitionReset {
            source: "speaker".into(),
        },
        now,
        &config,
    );
    state.apply(partial_event("u1", "speaker", "new session"), now, &config);

    assert_eq!(
        state
            .frame(now, &config)
            .unwrap()
            .content
            .without_original(),
        PresentationContent::Wrist(vec![WristMessage {
            original: None,
            speaker: None,
            text: "new session".into(),
            side: MessageSide::Left,
        }])
    );
}

#[test]
fn recognition_reset_clears_only_matching_wrist_source() {
    let now = Instant::now();
    let config = VrOverlayWristConfig {
        show_partials: true,
        include_microphone: true,
        ..Default::default()
    };
    let mut state = WristPresentation::default();
    state.apply(partial_event("u1", "speaker", "remote"), now, &config);
    state.apply(partial_event("u2", "microphone", "local"), now, &config);
    state.apply(
        PresentationEvent::RecognitionReset {
            source: "speaker".into(),
        },
        now,
        &config,
    );

    assert_eq!(
        state
            .frame(now, &config)
            .unwrap()
            .content
            .without_original(),
        PresentationContent::Wrist(vec![WristMessage {
            original: None,
            speaker: None,
            text: "local".into(),
            side: MessageSide::Right,
        }])
    );
}

#[test]
fn disabled_or_unknown_sources_are_not_presented() {
    let now = Instant::now();
    let config = VrOverlayHeadsetConfig::default();
    let mut state = HeadsetPresentation::default();
    let mut item = subtitle(1, "private");
    item.source = "microphone".into();
    state.apply(final_event(item), now, &config);
    assert!(state.frame(now, &config).is_none());
}

#[test]
fn native_translation_can_arrive_before_the_original() {
    let now = Instant::now();
    let snapshot = vrcs_core::LiveTranslation {
        source_utterance_id: None,
        completed_original: None,
        conversation_preview: None,
        speaker: None,
        utterance_id: "native-1".into(),
        text: String::new(),
        language: None,
        translation: "hello".into(),
        target_language: "en".into(),
    };
    let event = PresentationEvent::LiveTranslationUpdated {
        source: "speaker".into(),
        snapshot,
    };
    let config = VrOverlayHeadsetConfig {
        show_partials: true,
        show_translation_partials: true,
        ..Default::default()
    };
    let mut headset = HeadsetPresentation::default();
    headset.apply(event.clone(), now, &config);
    assert!(headset
        .partials
        .back()
        .is_some_and(|item| item.original.is_empty() && item.translations[0].text == "hello"));
    headset.apply(
        PresentationEvent::RecognitionCancelled {
            source: "speaker".into(),
            utterance_id: "native-1".into(),
        },
        now,
        &config,
    );
    headset.apply(event.clone(), now, &config);
    assert!(headset.partials.is_empty());
    let mut wrist = WristPresentation::default();
    let wrist_config = VrOverlayWristConfig {
        show_partials: true,
        show_translation_partials: false,
        ..Default::default()
    };
    wrist.apply(event, now, &wrist_config);
    assert!(wrist.partials[0].translations.is_empty());
}

#[test]
fn qwen_translation_delta_keeps_its_completed_original_in_bilingual_preview() {
    let now = Instant::now();
    let config = VrOverlayHeadsetConfig {
        show_partials: true,
        show_translation_partials: true,
        ..Default::default()
    };
    let wrist_config = VrOverlayWristConfig {
        show_partials: true,
        show_translation_partials: true,
        ..Default::default()
    };
    let mut headset = HeadsetPresentation::default();
    let mut wrist = WristPresentation::default();
    let final_event = PresentationEvent::Final {
        utterance_id: Some("qwen-source-1".into()),
        subtitle: subtitle(7, "Hello."),
    };
    let preview = PresentationEvent::LiveTranslationUpdated {
        source: "speaker".into(),
        snapshot: vrcs_core::LiveTranslation {
            utterance_id: "qwen-preview-1".into(),
            source_utterance_id: Some("qwen-source-1".into()),
            completed_original: None,
            conversation_preview: None,
            speaker: None,
            text: String::new(),
            language: Some("en".into()),
            translation: "你好".into(),
            target_language: "zh".into(),
        },
    };
    for event in [final_event, preview] {
        headset.apply(event.clone(), now, &config);
        wrist.apply(event, now, &wrist_config);
    }
    assert_eq!(
        headset
            .frame(now, &config)
            .unwrap()
            .content
            .without_original(),
        headset_content("Hello.\n你好".into())
    );
    assert_eq!(
        wrist
            .frame(now, &wrist_config)
            .unwrap()
            .content
            .without_original(),
        PresentationContent::Wrist(vec![WristMessage {
            original: None,
            speaker: None,
            text: "Hello.\n你好".into(),
            side: MessageSide::Left,
        }])
    );
}

#[test]
fn paragraph_breaks_do_not_create_extra_language_slots() {
    let now = Instant::now();
    let mut item = item_from_subtitle(subtitle(1, "First sentence.\nSecond sentence."), None, now);
    update_completed_translation(&mut item, translation("第一句。\n第二句。"), true);
    assert_eq!(
        display_text(&item, "bilingual", "all_languages", "\n"),
        "First sentence. Second sentence.\n第一句。 第二句。"
    );
}

#[test]
fn recognition_only_preview_keeps_its_original_and_never_pairs_different_utterances() {
    let now = Instant::now();
    let config = VrOverlayHeadsetConfig {
        show_partials: true,
        show_translation_partials: false,
        ..Default::default()
    };
    let mut state = HeadsetPresentation::default();
    let mut original = subtitle(7, "Hello.");
    original.translations.push(translation("你好。"));
    state.apply(
        PresentationEvent::Final {
            utterance_id: Some("source-1".into()),
            subtitle: original,
        },
        now,
        &config,
    );
    let preview = |id: &str, text: &str| PresentationEvent::LiveTranslationUpdated {
        source: "speaker".into(),
        snapshot: vrcs_core::LiveTranslation {
            utterance_id: format!("preview-{id}"),
            source_utterance_id: Some(id.into()),
            completed_original: None,
            conversation_preview: None,
            speaker: None,
            text: text.into(),
            language: Some("en".into()),
            translation: "unfinished translation must stay hidden".into(),
            target_language: "zh".into(),
        },
    };
    state.apply(preview("source-1", "Hello"), now, &config);
    assert_eq!(
        state
            .frame(now, &config)
            .unwrap()
            .content
            .without_original(),
        headset_content("Hello.".into())
    );
    state.apply(preview("source-2", "Next sentence"), now, &config);
    assert_eq!(
        state
            .frame(now, &config)
            .unwrap()
            .content
            .without_original(),
        headset_content("Next sentence".into())
    );
}

#[test]
fn late_translation_preview_uses_its_own_original_after_another_source_finishes() {
    let now = Instant::now();
    let config = VrOverlayHeadsetConfig {
        show_partials: true,
        show_translation_partials: true,
        ..Default::default()
    };
    let mut state = HeadsetPresentation::default();
    for (id, text) in [("source-1", "First."), ("source-2", "Second.")] {
        state.apply(
            PresentationEvent::Final {
                utterance_id: Some(id.into()),
                subtitle: subtitle(if id == "source-1" { 1 } else { 2 }, text),
            },
            now,
            &config,
        );
    }
    state.apply(
        PresentationEvent::LiveTranslationUpdated {
            source: "speaker".into(),
            snapshot: vrcs_core::LiveTranslation {
                utterance_id: "preview-1".into(),
                source_utterance_id: Some("source-1".into()),
                completed_original: None,
                conversation_preview: None,
                speaker: None,
                text: String::new(),
                language: Some("en".into()),
                translation: "第一句".into(),
                target_language: "zh".into(),
            },
        },
        now,
        &config,
    );
    assert_eq!(
        state
            .frame(now, &config)
            .unwrap()
            .content
            .without_original(),
        headset_content("First.\n第一句".into())
    );
}

#[test]
fn native_deltas_update_subtitle_previews_without_duplicating_the_stored_original() {
    let now = Instant::now();
    let headset_config = VrOverlayHeadsetConfig {
        show_partials: false,
        show_translation_partials: true,
        ..Default::default()
    };
    let wrist_config = VrOverlayWristConfig {
        show_partials: false,
        show_translation_partials: true,
        ..Default::default()
    };
    let preview = |text: &str| PresentationEvent::LiveTranslationUpdated {
        source: "speaker".into(),
        snapshot: vrcs_core::LiveTranslation {
            utterance_id: "qwen-preview-1".into(),
            source_utterance_id: Some("qwen-source-1".into()),
            completed_original: None,
            conversation_preview: Some(vrcs_core::LiveTranslationPreview {
                text: "long original ".repeat(30),
                translation: "long translation ".repeat(30),
            }),
            speaker: None,
            text: "hello".into(),
            language: Some("en".into()),
            translation: text.into(),
            target_language: "zh".into(),
        },
    };
    let mut headset = HeadsetPresentation::default();
    let mut wrist = WristPresentation::default();
    let original = PresentationEvent::Final {
        utterance_id: Some("qwen-source-1".into()),
        subtitle: subtitle(7, "hello"),
    };
    for event in [
        preview("你"),
        original,
        PresentationEvent::TranslationPartial {
            subtitle_id: 7,
            text: "你好".into(),
            target_language: "zh".into(),
            preferred: true,
        },
        preview("你好"),
    ] {
        headset.apply(event.clone(), now, &headset_config);
        wrist.apply(event, now, &wrist_config);
    }
    assert_eq!(
        headset
            .frame(now, &headset_config)
            .unwrap()
            .content
            .without_original(),
        headset_content("你好".into())
    );
    assert_eq!(
        wrist
            .frame(now, &wrist_config)
            .unwrap()
            .content
            .without_original(),
        PresentationContent::Wrist(vec![WristMessage {
            original: None,
            speaker: None,
            text: "你好".into(),
            side: MessageSide::Left
        }])
    );
    for event in [
        PresentationEvent::TranslationCompleted {
            subtitle_id: 7,
            translation: translation("你好，完整结果。"),
            preferred: true,
        },
        PresentationEvent::RecognitionCancelled {
            source: "speaker".into(),
            utterance_id: "qwen-preview-1".into(),
        },
    ] {
        headset.apply(event.clone(), now, &headset_config);
        wrist.apply(event, now, &wrist_config);
    }
    assert_eq!(
        headset
            .frame(now, &headset_config)
            .unwrap()
            .content
            .without_original(),
        headset_content("你好，完整结果。".into())
    );
    assert_eq!(
        wrist
            .frame(now, &wrist_config)
            .unwrap()
            .content
            .without_original(),
        PresentationContent::Wrist(vec![WristMessage {
            original: None,
            speaker: None,
            text: "你好，完整结果。".into(),
            side: MessageSide::Left
        }])
    );
}

#[test]
fn japanese_live_translation_preview_does_not_require_recognition_preview() {
    let now = Instant::now();
    let config = VrOverlayHeadsetConfig {
        show_partials: false,
        show_translation_partials: true,
        ..Default::default()
    };
    let event = PresentationEvent::LiveTranslationUpdated {
        source: "speaker".into(),
        snapshot: vrcs_core::LiveTranslation {
            source_utterance_id: None,
            completed_original: None,
            conversation_preview: None,
            speaker: None,
            utterance_id: "japanese-stream".into(),
            text: "日本語の音声を再生しています。".into(),
            language: Some("ja".into()),
            translation: "正在播放日语音频。".into(),
            target_language: "zh-Hans".into(),
        },
    };
    let mut state = HeadsetPresentation::default();
    state.apply(event.clone(), now, &config);
    assert_eq!(
        state
            .frame(now, &config)
            .unwrap()
            .content
            .without_original(),
        headset_content("正在播放日语音频。".into())
    );
    let config = VrOverlayWristConfig {
        show_partials: false,
        show_translation_partials: true,
        ..Default::default()
    };
    let mut wrist = WristPresentation::default();
    wrist.apply(event, now, &config);
    assert_eq!(
        wrist
            .frame(now, &config)
            .unwrap()
            .content
            .without_original(),
        PresentationContent::Wrist(vec![WristMessage {
            original: None,
            speaker: None,
            text: "正在播放日语音频。".into(),
            side: MessageSide::Left,
        }])
    );
}

#[test]
fn empty_live_original_does_not_hide_a_completed_japanese_subtitle() {
    let now = Instant::now();
    let config = VrOverlayHeadsetConfig {
        show_partials: true,
        show_translation_partials: false,
        ..Default::default()
    };
    let mut state = HeadsetPresentation::default();
    let mut original = subtitle(1, "日本語の字幕です。");
    original.language = Some("ja".into());
    state.apply(final_event(original), now, &config);
    state.apply(
        PresentationEvent::LiveTranslationUpdated {
            source: "speaker".into(),
            snapshot: vrcs_core::LiveTranslation {
                source_utterance_id: None,
                completed_original: None,
                conversation_preview: None,
                speaker: None,
                utterance_id: "next-japanese-stream".into(),
                text: String::new(),
                language: Some("ja".into()),
                translation: "下一句的译文".into(),
                target_language: "zh-Hans".into(),
            },
        },
        now,
        &config,
    );
    assert_eq!(
        state
            .frame(now, &config)
            .unwrap()
            .content
            .without_original(),
        headset_content("日本語の字幕です。".into())
    );
}

#[test]
fn expired_preview_does_not_hide_newer_completed_subtitles() {
    let now = Instant::now();
    let config = VrOverlayHeadsetConfig {
        display_seconds: 2.0,
        fade_seconds: 1.0,
        show_partials: true,
        ..Default::default()
    };
    let mut state = HeadsetPresentation::default();
    state.apply(
        partial_event("old-preview", "speaker", "古いプレビュー"),
        now,
        &config,
    );
    let later = now + Duration::from_secs(4);
    state.apply(final_event(subtitle(2, "新しい字幕")), later, &config);
    assert_eq!(
        state
            .frame(later, &config)
            .unwrap()
            .content
            .without_original(),
        headset_content("新しい字幕".into())
    );
}

#[test]
fn shared_translation_uses_group_context_only_on_the_last_sentence() {
    let now = Instant::now();
    let mut first = item_from_subtitle(subtitle(1, "Hello."), None, now);
    let mut last = item_from_subtitle(subtitle(2, "How are you?"), None, now);
    let mut translated = translation("你好，最近怎么样？");
    translated.source_group = Some(vrcs_core::TranslationSourceGroup {
        subtitle_ids: vec![1, 2],
        text: "Hello. How are you?".into(),
    });
    assert!(!update_completed_translation(
        &mut first,
        translated.clone(),
        true
    ));
    assert!(update_completed_translation(&mut last, translated, true));
    assert_eq!(
        display_text(&first, "bilingual", "preferred_only", "\n"),
        "Hello."
    );
    assert_eq!(
        display_text(&last, "bilingual", "preferred_only", "\n"),
        "Hello. How are you?\n你好，最近怎么样？"
    );
    assert_eq!(
        display_text(&last, "original", "preferred_only", "\n"),
        "How are you?"
    );
}

#[test]
fn diarized_final_has_a_speaker_label_with_both_previews_disabled() {
    let now = Instant::now();
    let mut item = subtitle(1, "Hello.");
    item.speaker = Some(vrcs_core::SpeakerIdentity {
        id: "qwen-session-1".into(),
        index: 1,
        display_name: None,
        user_id: None,
    });
    let config = VrOverlayHeadsetConfig {
        show_partials: false,
        show_translation_partials: false,
        ..Default::default()
    };
    let event = final_event(item);
    let mut headset = HeadsetPresentation::default();
    headset.apply(event.clone(), now, &config);
    assert_eq!(
        headset
            .frame(now, &config)
            .unwrap()
            .content
            .without_original(),
        PresentationContent::Headset(vec![WristMessage {
            original: None,
            text: "Hello.".into(),
            side: MessageSide::Left,
            speaker: Some("#2".into())
        }])
    );
    let config = VrOverlayWristConfig {
        show_partials: false,
        show_translation_partials: false,
        ..Default::default()
    };
    let mut wrist = WristPresentation::default();
    wrist.apply(event, now, &config);
    let frame = wrist.frame(now, &config).unwrap();
    let PresentationContent::Wrist(messages) = frame.content.without_original() else {
        panic!()
    };
    assert_eq!(messages[0].text, "Hello.");
    assert_eq!(messages[0].speaker.as_deref(), Some("#2"));
}

#[test]
fn diarized_live_preview_and_completed_translation_label_both_overlays() {
    let now = Instant::now();
    let speaker = vrcs_core::SpeakerIdentity {
        id: "qwen-session-1".into(),
        index: 1,
        display_name: None,
        user_id: None,
    };
    let event = PresentationEvent::LiveTranslationUpdated {
        source: "speaker".into(),
        snapshot: vrcs_core::LiveTranslation {
            utterance_id: "qwen-preview-1".into(),
            source_utterance_id: Some("qwen-source-1".into()),
            completed_original: None,
            conversation_preview: None,
            speaker: Some(speaker.clone()),
            text: "Hello.".into(),
            language: Some("en".into()),
            translation: "你好。".into(),
            target_language: "zh".into(),
        },
    };
    let headset_config = VrOverlayHeadsetConfig {
        show_partials: true,
        show_translation_partials: true,
        ..Default::default()
    };
    let wrist_config = VrOverlayWristConfig {
        show_partials: true,
        show_translation_partials: true,
        ..Default::default()
    };
    let mut headset = HeadsetPresentation::default();
    let mut wrist = WristPresentation::default();
    headset.apply(event.clone(), now, &headset_config);
    wrist.apply(event, now, &wrist_config);

    let assert_text = |headset: &HeadsetPresentation, wrist: &WristPresentation| {
        assert_eq!(
            headset
                .frame(now, &headset_config)
                .unwrap()
                .content
                .without_original(),
            PresentationContent::Headset(vec![WristMessage {
                original: None,
                text: "Hello.\n你好。".into(),
                side: MessageSide::Left,
                speaker: Some("#2".into())
            }])
        );
        let PresentationContent::Wrist(messages) = wrist
            .frame(now, &wrist_config)
            .unwrap()
            .content
            .without_original()
        else {
            panic!()
        };
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text, "Hello.\n你好。");
        assert_eq!(messages[0].speaker.as_deref(), Some("#2"));
    };
    assert_text(&headset, &wrist);

    let mut item = subtitle(1, "Hello.");
    item.speaker = Some(speaker);
    let event = PresentationEvent::Final {
        utterance_id: Some("qwen-source-1".into()),
        subtitle: item,
    };
    headset.apply(event.clone(), now, &headset_config);
    wrist.apply(event, now, &wrist_config);
    let event = PresentationEvent::TranslationCompleted {
        subtitle_id: 1,
        translation: translation("你好。"),
        preferred: true,
    };
    headset.apply(event.clone(), now, &headset_config);
    wrist.apply(event, now, &wrist_config);
    assert_text(&headset, &wrist);
}

#[test]
fn independent_player_lanes_names_limits_and_cancellation() {
    let now = Instant::now();
    let headset_config = VrOverlayHeadsetConfig {
        show_partials: true,
        include_microphone: true,
        ..Default::default()
    };
    let wrist_config = VrOverlayWristConfig {
        show_partials: true,
        include_microphone: true,
        max_entries: 3,
        ..Default::default()
    };
    let mut headset = HeadsetPresentation::default();
    let mut wrist = WristPresentation::default();
    headset.set_player_limit(Some(4));
    wrist.set_player_limit(Some(4));
    let preview = |index: u64, text: &str| PresentationEvent::RecognitionPartial {
        utterance_id: format!("player-{index}-sentence"),
        source: "speaker".into(),
        text: text.into(),
        language: Some("en".into()),
        speaker: Some(vrcs_core::SpeakerIdentity {
            id: format!("player-{index}"),
            index,
            display_name: Some(format!("Player {index}")),
            user_id: None,
        }),
    };
    for index in 0..8 {
        let event = preview(index, &format!("Speech {index}"));
        headset.apply(event.clone(), now, &headset_config);
        wrist.apply(event, now, &wrist_config);
    }
    let get_messages = |content| match content {
        PresentationContent::Headset(messages) | PresentationContent::Wrist(messages) => messages,
    };
    for content in [
        headset
            .frame(now, &headset_config)
            .unwrap()
            .content
            .without_original(),
        wrist
            .frame(now, &wrist_config)
            .unwrap()
            .content
            .without_original(),
    ] {
        let messages = get_messages(content);
        assert_eq!(
            messages.len(),
            4,
            "history limit must not discard visible active players"
        );
        for (index, message) in messages.iter().enumerate() {
            assert_eq!(message.speaker, Some(format!("Player {index}")));
        }
    }
    // Updating one player does not change the visible set/order or another player's text.
    let event = preview(2, "Updated independently");
    headset.apply(event.clone(), now, &headset_config);
    wrist.apply(event, now, &wrist_config);
    let event = PresentationEvent::SpeakerUpdated {
        speaker: vrcs_core::SpeakerIdentity {
            id: "player-2".into(),
            index: 2,
            display_name: Some("New name".into()),
            user_id: None,
        },
    };
    headset.apply(event.clone(), now, &headset_config);
    wrist.apply(event, now, &wrist_config);
    for content in [
        headset
            .frame(now, &headset_config)
            .unwrap()
            .content
            .without_original(),
        wrist
            .frame(now, &wrist_config)
            .unwrap()
            .content
            .without_original(),
    ] {
        let messages = get_messages(content);
        assert_eq!(messages[0].text, "Speech 0");
        assert_eq!(messages[2].text, "Updated independently");
        assert_eq!(messages[2].speaker.as_deref(), Some("New name"));
    }
    headset.set_player_limit(Some(0));
    wrist.set_player_limit(Some(0));
    let event = partial_event("my-microphone", "microphone", "My speech");
    headset.apply(event.clone(), now, &headset_config);
    wrist.apply(event, now, &wrist_config);
    assert_eq!(
        get_messages(
            headset
                .frame(now, &headset_config)
                .unwrap()
                .content
                .without_original()
        )
        .len(),
        9
    );
    assert_eq!(
        get_messages(
            wrist
                .frame(now, &wrist_config)
                .unwrap()
                .content
                .without_original()
        )
        .len(),
        9
    );
    let event = PresentationEvent::RecognitionCancelled {
        source: "speaker".into(),
        utterance_id: "player-2-sentence".into(),
    };
    headset.apply(event.clone(), now, &headset_config);
    wrist.apply(event, now, &wrist_config);
    assert_eq!(
        get_messages(
            headset
                .frame(now, &headset_config)
                .unwrap()
                .content
                .without_original()
        )
        .len(),
        8
    );
    assert_eq!(
        get_messages(
            wrist
                .frame(now, &wrist_config)
                .unwrap()
                .content
                .without_original()
        )
        .len(),
        8
    );
    // Reducing the limit doesn't consume a slot for the user's microphone.
    headset.set_player_limit(Some(1));
    wrist.set_player_limit(Some(1));
    assert_eq!(
        get_messages(
            headset
                .frame(now, &headset_config)
                .unwrap()
                .content
                .without_original()
        )
        .len(),
        2
    );
    assert_eq!(
        get_messages(
            wrist
                .frame(now, &wrist_config)
                .unwrap()
                .content
                .without_original()
        )
        .len(),
        2
    );
}

#[test]
fn caption_candidates_keep_original_identity_and_language_for_shared_visibility() {
    let now = Instant::now();
    let headset_config = VrOverlayHeadsetConfig {
        show_partials: false,
        show_translation_partials: true,
        ..Default::default()
    };
    let wrist_config = VrOverlayWristConfig {
        show_partials: false,
        show_translation_partials: true,
        ..Default::default()
    };
    let mut headset = HeadsetPresentation::default();
    let mut wrist = WristPresentation::default();
    headset.set_player_limit(Some(0));
    wrist.set_player_limit(Some(0));
    let mut value = subtitle(1, "这是中文原文");
    value.language = None;
    value.speaker = Some(vrcs_core::SpeakerIdentity {
        id: "track-a".into(),
        index: 0,
        display_name: Some("Alice".into()),
        user_id: None,
    });
    let mut translated = translation("English translation only");
    translated.source_language = Some("zh-CN".into());
    translated.target_language = "en".into();
    value.translations.push(translated);
    let event = PresentationEvent::Final {
        utterance_id: Some("original-1".into()),
        subtitle: value,
    };
    headset.apply(event.clone(), now, &headset_config);
    wrist.apply(event, now, &wrist_config);
    let inspect = |headset: &HeadsetPresentation, wrist: &WristPresentation, live| {
        for frame in [
            headset
                .frame_with_translation_display(now, &headset_config, "all_languages")
                .unwrap(),
            wrist
                .frame_with_translation_display(now, &wrist_config, "all_languages")
                .unwrap(),
        ] {
            let (PresentationContent::Headset(messages) | PresentationContent::Wrist(messages)) =
                frame.content;
            let message = messages
                .iter()
                .find(|message| {
                    message
                        .original
                        .as_ref()
                        .is_some_and(|original| original.live == live)
                })
                .unwrap();
            assert_eq!(message.text, "English translation only");
            let original = message.original.as_ref().unwrap();
            assert_eq!(original.text, "这是中文原文");
            assert_eq!(original.source, "speaker");
            assert_eq!(original.track_id.as_deref(), Some("track-a"));
            assert_eq!(original.translation_sources, ["zh-CN"]);
            assert_eq!(original.live, live);
            let wire = serde_json::to_value(message).unwrap();
            assert_eq!(wire["original"]["text"], "这是中文原文");
        }
    };
    inspect(&headset, &wrist, false);
    let event = PresentationEvent::LiveTranslationUpdated {
        source: "speaker".into(),
        snapshot: vrcs_core::LiveTranslation {
            utterance_id: "qwen-preview-1".into(),
            source_utterance_id: Some("original-1".into()),
            text: String::new(),
            language: None,
            completed_original: None,
            conversation_preview: None,
            speaker: Some(vrcs_core::SpeakerIdentity {
                id: "track-a".into(),
                index: 0,
                display_name: Some("Alice".into()),
                user_id: None,
            }),
            translation: "English translation only".into(),
            target_language: "en".into(),
        },
    };
    headset.apply(event.clone(), now, &headset_config);
    wrist.apply(event, now, &wrist_config);
    // The source may be absent from a late translation delta, but remains
    // available to the language filter independently of the displayed text.
    inspect(&headset, &wrist, true);
}

#[test]
fn shared_surface_selection_receives_candidates_before_history_and_player_limits() {
    let now = Instant::now();
    let headset_config = VrOverlayHeadsetConfig::default();
    let wrist_config = VrOverlayWristConfig {
        max_entries: 3,
        ..Default::default()
    };
    let mut headset = HeadsetPresentation::default();
    let mut wrist = WristPresentation::default();
    headset.set_player_limit(Some(0));
    wrist.set_player_limit(Some(0));
    for index in 0..12 {
        let mut value = subtitle(index, &format!("Candidate {index}"));
        value.speaker = Some(vrcs_core::SpeakerIdentity {
            id: "same-track".into(),
            index: 0,
            display_name: None,
            user_id: None,
        });
        let event = PresentationEvent::Final {
            utterance_id: Some(format!("utterance-{index}")),
            subtitle: value,
        };
        headset.apply(event.clone(), now, &headset_config);
        wrist.apply(event, now, &wrist_config);
    }
    for frame in [
        headset
            .frame_with_translation_display(now, &headset_config, "all_languages")
            .unwrap(),
        wrist
            .frame_with_translation_display(now, &wrist_config, "all_languages")
            .unwrap(),
    ] {
        let (PresentationContent::Headset(messages) | PresentationContent::Wrist(messages)) =
            frame.content;
        assert_eq!(
            messages.len(),
            12,
            "hiding newer source languages must not discard older visible candidates"
        );
        assert!(messages.iter().all(|message| message.original.is_some()));
    }
}

#[test]
fn corrected_translation_source_language_replaces_its_previous_detection() {
    let mut value = subtitle(1, "123");
    value.language = None;
    let mut item = item_from_subtitle(value, Some("source".into()), Instant::now());
    let mut translated = translation("Translation");
    translated.target_language = "en".into();
    translated.source_language = Some("fr".into());
    update_completed_translation(&mut item, translated.clone(), true);
    translated.source_language = Some("zh".into());
    update_completed_translation(&mut item, translated, true);
    assert_eq!(
        caption_original(&item, false, None).translation_sources,
        ["zh"]
    );
    item.visibility_original = Some("完整的中文预览".into());
    assert_eq!(caption_original(&item, true, None).text, "完整的中文预览");
}
