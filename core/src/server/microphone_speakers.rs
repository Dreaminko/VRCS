use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use crate::microphone_speakers::{SpeakerSelection, SpeakerSnapshot};

use super::{api_error, ApiResult, OutputContext};

pub(super) async fn get(State(state): State<OutputContext>) -> Json<SpeakerSnapshot> {
    Json(
        state
            .content
            .subtitle_output
            .microphone_speakers()
            .snapshot(),
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Update {
    generation: String,
    revision: u64,
    selections: Vec<SpeakerSelection>,
}

pub(super) async fn update(
    State(state): State<OutputContext>,
    Json(update): Json<Update>,
) -> ApiResult<Json<SpeakerSnapshot>> {
    let output = &state.content.subtitle_output;
    let speakers = output.microphone_speakers();
    let enabled = speakers.snapshot().enabled;
    let (snapshot, cancelled) = speakers
        .apply(
            &update.generation,
            update.revision,
            enabled,
            &update.selections,
        )
        .map_err(|reason| {
            api_error(
                if matches!(reason, "stale" | "unknown") {
                    StatusCode::CONFLICT
                } else {
                    StatusCode::UNPROCESSABLE_ENTITY
                },
                format!("microphone_speakers.{reason}"),
                "Microphone speaker selection changed or is unavailable; refresh and try again",
            )
        })?;
    for id in cancelled {
        output.asr_cancelled(&id, "microphone", "speaker_filtered");
    }
    Ok(Json(snapshot))
}
