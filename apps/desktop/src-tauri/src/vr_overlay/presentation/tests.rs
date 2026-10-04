use super::*;

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
                    headset.frame(now, &headset_config).unwrap().content,
                    PresentationContent::Headset(expected.into()),
                    "original={show_original}, translation={show_translation}",
                );
                assert_eq!(
                    wrist.frame(now, &wrist_config).unwrap().content,
                    PresentationContent::Wrist(vec![WristMessage {
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
                headset.frame(now, &headset_config).unwrap().content,
                PresentationContent::Headset(expected.into())
            );
            let PresentationContent::Wrist(messages) =
                wrist.frame(now, &wrist_config).unwrap().content
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
            assert!(headset.partial.is_none() && wrist.partials.is_empty());
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
    }
}

fn translation(text: &str) -> vrcs_core::SubtitleTranslation {
    translation_in("zh", text)
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
    assert_eq!(fading.content, PresentationContent::Headset("hello".into()));
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
        state.frame(now, &config).unwrap().content,
        PresentationContent::Headset("hello\n你好".into())
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
            .content,
        PresentationContent::Headset("hello\n你好".into())
    );
    assert_eq!(
        headset
            .frame_with_translation_display(now, &headset_config, "all_languages")
            .unwrap()
            .content,
        PresentationContent::Headset("hello\n你好\nこんにちは".into())
    );
    assert_eq!(
        wrist
            .frame_with_translation_display(now, &wrist_config, "preferred_only")
            .unwrap()
            .content,
        PresentationContent::Wrist(vec![WristMessage {
            text: "你好".into(),
            side: MessageSide::Left,
        }])
    );
    assert_eq!(
        wrist
            .frame_with_translation_display(now, &wrist_config, "all_languages")
            .unwrap()
            .content,
        PresentationContent::Wrist(vec![WristMessage {
            text: "你好\nこんにちは".into(),
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
            .content,
        PresentationContent::Wrist(vec![WristMessage {
            text: "你好\nこんにちは".into(),
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
            .content,
        PresentationContent::Headset("你好".into())
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
        headset.frame(now, &headset_config).unwrap().content,
        PresentationContent::Headset("hello".into())
    );

    let wrist_config = VrOverlayWristConfig {
        ..Default::default()
    };
    let mut item = subtitle(7, "hello");
    item.translations.push(translation);
    let mut wrist = WristPresentation::default();
    wrist.apply(final_event(item), now, &wrist_config);
    assert_eq!(
        wrist.frame(now, &wrist_config).unwrap().content,
        PresentationContent::Wrist(vec![WristMessage {
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
        state.frame(now, &config).unwrap().content,
        PresentationContent::Wrist(vec![
            WristMessage {
                text: "line3".into(),
                side: MessageSide::Left,
            },
            WristMessage {
                text: "line4".into(),
                side: MessageSide::Left,
            },
            WristMessage {
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

    let PresentationContent::Wrist(messages) = state.frame(now, &config).unwrap().content else {
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

    let PresentationContent::Wrist(messages) = state.frame(now, &config).unwrap().content else {
        panic!("expected wrist messages");
    };
    assert_eq!(messages[0].side, MessageSide::Left);
    assert_eq!(messages[1].side, MessageSide::Right);
    assert_eq!(messages[2].side, MessageSide::Right);
}

#[test]
fn recognition_partials_are_visible_only_when_enabled() {
    let now = Instant::now();
    let mut headset_config = VrOverlayHeadsetConfig::default();
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
        headset.frame(now, &headset_config).unwrap().content,
        PresentationContent::Headset("hello".into())
    );

    let wrist_config = VrOverlayWristConfig {
        show_partials: true,
        ..Default::default()
    };
    let mut wrist = WristPresentation::default();
    wrist.apply(partial_event("u1", "speaker", "hello"), now, &wrist_config);
    assert_eq!(
        wrist.frame(now, &wrist_config).unwrap().content,
        PresentationContent::Wrist(vec![WristMessage {
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
        state.frame(now, &config).unwrap().content,
        PresentationContent::Headset("final".into())
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

    let PresentationContent::Wrist(messages) = state.frame(now, &config).unwrap().content else {
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
        state.frame(now, &config).unwrap().content,
        PresentationContent::Wrist(vec![WristMessage {
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
        state.frame(now, &config).unwrap().content,
        PresentationContent::Wrist(vec![WristMessage {
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
        .partial
        .as_ref()
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
    assert!(headset.partial.is_none());
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
        headset.frame(now, &config).unwrap().content,
        PresentationContent::Headset("Hello.\n你好".into())
    );
    assert_eq!(
        wrist.frame(now, &wrist_config).unwrap().content,
        PresentationContent::Wrist(vec![WristMessage {
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
        state.frame(now, &config).unwrap().content,
        PresentationContent::Headset("Hello.".into())
    );
    state.apply(preview("source-2", "Next sentence"), now, &config);
    assert_eq!(
        state.frame(now, &config).unwrap().content,
        PresentationContent::Headset("Next sentence".into())
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
        state.frame(now, &config).unwrap().content,
        PresentationContent::Headset("First.\n第一句".into())
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
        headset.frame(now, &headset_config).unwrap().content,
        PresentationContent::Headset("你好".into())
    );
    assert_eq!(
        wrist.frame(now, &wrist_config).unwrap().content,
        PresentationContent::Wrist(vec![WristMessage {
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
        headset.frame(now, &headset_config).unwrap().content,
        PresentationContent::Headset("你好，完整结果。".into())
    );
    assert_eq!(
        wrist.frame(now, &wrist_config).unwrap().content,
        PresentationContent::Wrist(vec![WristMessage {
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
        state.frame(now, &config).unwrap().content,
        PresentationContent::Headset("正在播放日语音频。".into())
    );
    let config = VrOverlayWristConfig {
        show_partials: false,
        show_translation_partials: true,
        ..Default::default()
    };
    let mut wrist = WristPresentation::default();
    wrist.apply(event, now, &config);
    assert_eq!(
        wrist.frame(now, &config).unwrap().content,
        PresentationContent::Wrist(vec![WristMessage {
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
        state.frame(now, &config).unwrap().content,
        PresentationContent::Headset("日本語の字幕です。".into())
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
        state.frame(later, &config).unwrap().content,
        PresentationContent::Headset("新しい字幕".into())
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
        headset.frame(now, &config).unwrap().content,
        PresentationContent::Headset("[2] Hello.".into())
    );
    let config = VrOverlayWristConfig {
        show_partials: false,
        show_translation_partials: false,
        ..Default::default()
    };
    let mut wrist = WristPresentation::default();
    wrist.apply(event, now, &config);
    let frame = wrist.frame(now, &config).unwrap();
    let PresentationContent::Wrist(messages) = frame.content else {
        panic!()
    };
    assert_eq!(messages[0].text, "[2] Hello.");
}

#[test]
fn diarized_live_preview_and_completed_translation_label_both_overlays() {
    let now = Instant::now();
    let speaker = vrcs_core::SpeakerIdentity {
        id: "qwen-session-1".into(),
        index: 1,
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
            headset.frame(now, &headset_config).unwrap().content,
            PresentationContent::Headset("[2] Hello.\n你好。".into())
        );
        let PresentationContent::Wrist(messages) = wrist.frame(now, &wrist_config).unwrap().content
        else {
            panic!()
        };
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text, "[2] Hello.\n你好。");
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
