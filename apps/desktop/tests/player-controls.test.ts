import assert from "node:assert/strict";
import test from "node:test";
import { collectPlayerSpeakers, applyPlayerSelectionAction, applyPlayerVisibilityAction } from "../src/subtitles/player-controls.ts";
import type { Settings } from "../src/settings/types.ts";
import type { Subtitle } from "../src/subtitles/types.ts";

const alice = { id: "vrc:a", index: 0, user_id: "usr_00000000-0000-4000-8000-000000000001", display_name: "Alice" };
const bob = { id: "vrc:b", index: 1, user_id: "usr_00000000-0000-4000-8000-000000000002", display_name: "Alice" };
const settings = () => ({ audio: { output: { mode: "vrchat_players", player_audio: {
  selected_tracks: [bob.user_id], selected_user_names: { [bob.user_id]: "Offline Bob" }, max_players: 3,
} } } }) as Settings;

test("current selection merges accounts and preserves offline names and unrelated options", () => {
  const next = applyPlayerSelectionAction(settings(), { select_current: null }, [alice], [alice]);
  assert.deepEqual(next.audio.output.player_audio?.selected_tracks, [bob.user_id, alice.user_id]);
  assert.deepEqual(next.audio.output.player_audio?.selected_user_names, { [bob.user_id]: "Offline Bob", [alice.user_id]: "Alice" });
  assert.equal(next.audio.output.player_audio?.max_players, 3);
  assert.deepEqual(applyPlayerSelectionAction(next, { set_processing_mode: "selected" }, [], []).audio.output.player_audio?.selected_tracks, []);
  assert.equal(applyPlayerSelectionAction(next, { set_processing_mode: "all" }, [], []).audio.output.player_audio?.selected_tracks, null);
});

test("unknown identities and stale actions cannot enter the processing whitelist", () => {
  assert.throws(() => applyPlayerSelectionAction(settings(), { set_user_selected: { user_id: "unknown", selected: true } }, [alice], [alice]));
  const all = applyPlayerSelectionAction(settings(), { set_processing_mode: "all" }, [], []);
  assert.throws(() => applyPlayerSelectionAction(all, { set_user_selected: { user_id: alice.user_id, selected: true } }, [alice], [alice]));
  const full = settings();
  full.audio.output.player_audio!.selected_tracks = Array.from({ length: 512 }, (_, i) => `usr_00000000-0000-4000-8000-${String(i + 10).padStart(12, "0")}`);
  assert.throws(() => applyPlayerSelectionAction(full, { select_current: null }, [alice], [alice]), /512/);
});

test("current metadata wins without merging same-name accounts or separate tracks", () => {
  const historical = { ...alice, display_name: "Old Alice" };
  const rows = [{ source: "speaker", speaker: historical }] as Subtitle[];
  const speakers = collectPlayerSpeakers(rows, [alice, { ...alice, id: "vrc:a2" }], [bob.user_id], { [bob.user_id]: "Offline Bob" });
  assert.deepEqual(speakers.map(s => [s.id, s.display_name]), [["vrc:a", "Alice"], ["vrc:a2", "Alice"], [`saved-user:${bob.user_id}`, "Offline Bob"]]);
  const next = applyPlayerSelectionAction(settings(), { select_current: null }, [historical, bob], [alice]);
  assert.equal(next.audio.output.player_audio?.selected_user_names?.[bob.user_id], "Offline Bob");
  assert.equal(next.audio.output.player_audio?.selected_user_names?.[alice.user_id], "Alice");
});

test("a current track with pending metadata keeps its loaded historical name", () => {
  const rows = [{ source: "speaker", speaker: alice }] as Subtitle[];
  const [speaker] = collectPlayerSpeakers(rows, [{ ...alice, display_name: undefined }], [], {});
  assert.equal(speaker.display_name, "Alice");
});

test("track hiding is independent of language filtering and follow-default differs from an empty list", () => {
  const initial = { hiddenTracks: [alice.id], hiddenSourceLanguages: null };
  const manual = applyPlayerVisibilityAction(initial, { set_follow_default: false }, ["zh", "ja"]);
  assert.deepEqual(manual, { hiddenTracks: [alice.id], hiddenSourceLanguages: ["zh", "ja"] });
  assert.deepEqual(applyPlayerVisibilityAction(manual, { show_all_tracks: null }, []), { hiddenTracks: [], hiddenSourceLanguages: ["zh", "ja"] });
  const clear = applyPlayerVisibilityAction(manual, { clear_hidden_languages: null }, []);
  assert.deepEqual(clear, { hiddenTracks: [alice.id], hiddenSourceLanguages: [] });
  assert.equal(applyPlayerVisibilityAction(clear, { set_follow_default: true }, []).hiddenSourceLanguages, null);
  assert.throws(() => applyPlayerVisibilityAction(initial, { set_source_language_hidden: { language: "en", hidden: true } }, []));
});
