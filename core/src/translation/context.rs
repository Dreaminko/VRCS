use std::collections::HashMap;

use crate::config::{TranslationPromptConfig, VrchatContextConfig};
use crate::db::Database;
use crate::models::SpeakerIdentity;
use crate::room_context::{RoomContextAvailability, RoomContextSnapshot};
use crate::vrcx::VrcxIntegration;

use super::prompt::{RoomSpeakerRef, RoomTranslationContext};
use super::TranslationContextEntry;

const MAX_PROMPT_PARTICIPANTS: usize = 16;

pub(crate) fn room_translation_context(
    snapshot: &RoomContextSnapshot,
    vrchat_context: &VrchatContextConfig,
    current_speaker: Option<&SpeakerIdentity>,
) -> Option<RoomTranslationContext> {
    if !vrchat_context.include_in_translation_context
        || snapshot.availability == RoomContextAvailability::Unavailable
    {
        return None;
    }

    let world_name = vrchat_context
        .include_world_name
        .then(|| snapshot.world_name.clone())
        .flatten();
    let current = snapshot
        .member_for_speaker(current_speaker)
        .map(|member| RoomSpeakerRef {
            label: member.label.clone(),
            display_name: member.display_name.clone(),
        });

    let mut speaker_labels = HashMap::new();
    let mut participants = Vec::new();
    for member in snapshot.members.iter().take(MAX_PROMPT_PARTICIPANTS) {
        if let Some(uid) = &member.user_id {
            speaker_labels.insert(uid.clone(), member.label.clone());
        }
        speaker_labels.insert(member.speaker_key.clone(), member.label.clone());
        for track in &member.track_ids {
            speaker_labels.insert(track.clone(), member.label.clone());
        }
        participants.push(RoomSpeakerRef {
            label: member.label.clone(),
            display_name: member.display_name.clone(),
        });
    }
    if let Some(member) = snapshot.member_for_speaker(current_speaker) {
        if let Some(uid) = &member.user_id {
            speaker_labels.insert(uid.clone(), member.label.clone());
        }
        speaker_labels.insert(member.speaker_key.clone(), member.label.clone());
        for track in &member.track_ids {
            speaker_labels.insert(track.clone(), member.label.clone());
        }
    }

    let context = RoomTranslationContext {
        world_name,
        current_speaker: current,
        participants,
        speaker_labels,
    };
    (!context.is_empty()).then_some(context)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn load_translation_context(
    database: &Database,
    prompt: &TranslationPromptConfig,
    exclude_subtitle_id: Option<i64>,
    vrcx: &VrcxIntegration,
    include_vrcx: bool,
    vrchat_context: &VrchatContextConfig,
    snapshot: &RoomContextSnapshot,
    current_speaker: Option<&SpeakerIdentity>,
) -> (Vec<TranslationContextEntry>, Option<RoomTranslationContext>) {
    let native_active = vrchat_context.include_in_translation_context
        && snapshot.availability != RoomContextAvailability::Unavailable;
    let scope_id = native_active
        .then_some(snapshot.scope_id.as_deref())
        .flatten();
    let mut entries = database
        .recent_translation_context_scoped(prompt, exclude_subtitle_id, scope_id)
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "translation context could not be loaded");
            Vec::new()
        });
    let room_context = room_translation_context(snapshot, vrchat_context, current_speaker);
    if include_vrcx && !native_active {
        if let Some(entry) = vrcx.translation_context_entry() {
            entries.push(entry);
        }
    }
    (entries, room_context)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Subtitle;
    use crate::room_context::{RoomContextTier, RoomMember};
    use crate::vrchat_chatbox::TaskValidity;
    use tokio::sync::watch;

    fn snapshot_with_members() -> RoomContextSnapshot {
        RoomContextSnapshot {
            revision: 1,
            scope_id: Some("vrc-room:1:2:3".into()),
            world_name: Some("Starlight Cafe".into()),
            availability: RoomContextAvailability::Ready,
            tier: RoomContextTier::NetworkPlayerAudio,
            members: vec![RoomMember {
                speaker_key: "usr_a".into(),
                label: "p1".into(),
                display_name: "Alice".into(),
                user_id: Some("usr_a".into()),
                track_ids: vec!["track-a".into()],
                active: true,
                selected: true,
                distance_cm: Some(120),
                order: 0,
            }],
            validity: TaskValidity::new(),
        }
    }

    #[test]
    fn respects_toggles_world_name_cap_and_vrcx_dedup() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(&dir.path().join("ctx.db")).unwrap();
        db.add_subtitle_in_scope(
            &Subtitle {
                id: None,
                conversation_id: None,
                speaker: None,
                text: "room line".into(),
                language: None,
                started_at: None,
                ended_at: None,
                source: "speaker".into(),
                created_at: "2026-08-14T00:00:00Z".into(),
                translations: Vec::new(),
            },
            Some("vrc-room:1:2:3"),
            None,
        )
        .unwrap();
        db.add_subtitle_in_scope(
            &Subtitle {
                id: None,
                conversation_id: None,
                speaker: None,
                text: "other room".into(),
                language: None,
                started_at: None,
                ended_at: None,
                source: "speaker".into(),
                created_at: "2026-08-14T00:00:01Z".into(),
                translations: Vec::new(),
            },
            Some("vrc-room:other"),
            None,
        )
        .unwrap();

        let vrcx = VrcxIntegration::new(watch::channel(false).1);
        let prompt = TranslationPromptConfig {
            context_enabled: true,
            include_speaker: true,
            ..Default::default()
        };
        let snapshot = snapshot_with_members();
        let speaker = SpeakerIdentity {
            id: "track-a".into(),
            index: 0,
            display_name: Some("Alice".into()),
            user_id: Some("usr_a".into()),
        };

        let disabled = VrchatContextConfig::default();
        let (hist_off, room_off) = load_translation_context(
            &db,
            &prompt,
            None,
            &vrcx,
            false,
            &disabled,
            &snapshot,
            Some(&speaker),
        );
        assert!(room_off.is_none());
        assert_eq!(hist_off.len(), 2);

        let no_world = VrchatContextConfig {
            include_in_asr_context: false,
            include_in_translation_context: true,
            include_world_name: false,
        };
        let (hist_on, room_on) = load_translation_context(
            &db,
            &prompt,
            None,
            &vrcx,
            true,
            &no_world,
            &snapshot,
            Some(&speaker),
        );
        assert_eq!(hist_on.len(), 1);
        assert_eq!(hist_on[0].text, "room line");
        let room_on = room_on.unwrap();
        assert_eq!(room_on.world_name, None);
        assert_eq!(room_on.current_speaker.unwrap().label, "p1");
    }
}
