import assert from "node:assert/strict";
import test from "node:test";
import { collectPlayerSpeakers, applyPlayerProximity, applyPlayerSourceAction, applyPlayerSourceMode, applyPlayerVisibilityAction, sourceMode, sortPlayerSpeakersByDistance } from "../src/subtitles/player-controls.ts";
import type { Settings } from "../src/settings/types.ts";
import type { Subtitle } from "../src/subtitles/types.ts";

const alice = { id: "vrc:a", index: 0, user_id: "usr_00000000-0000-4000-8000-000000000001", display_name: "Alice" };
const bob = { id: "vrc:b", index: 1, user_id: "usr_00000000-0000-4000-8000-000000000002", display_name: "Alice" };
const settings = () => ({ audio: { output: { mode: "vrchat_players", player_audio: {
  selected_user_names: { [bob.user_id]: "Offline Bob" }, max_players: 3,
} } } }) as Settings;

test("distance settings preserve the existing player audio configuration", () => {
  const next = applyPlayerProximity(settings(), 10);
  assert.deepEqual(next.audio.output.player_audio?.proximity, { enabled: true, radius_m: 10 });
  assert.equal(next.audio.output.player_audio?.max_players, 3);
  assert.deepEqual(next.audio.output.player_audio?.selected_user_names, { [bob.user_id]: "Offline Bob" });
});

test("a zero distance switches filtering off while a positive one enables it", () => {
  assert.deepEqual(applyPlayerProximity(settings(), 0).audio.output.player_audio?.proximity, { enabled: false, radius_m: 0 });
  assert.deepEqual(applyPlayerProximity(settings(), 2.5).audio.output.player_audio?.proximity, { enabled: true, radius_m: 2.5 });
});

test("everyone mode stores only the sources a player switched off", () => {
  assert.equal(sourceMode(settings().audio.output.player_audio), "allowlist", "settings without a mode stay on the allow list");
  const everyone = applyPlayerSourceMode(settings(), "everyone");
  assert.equal(sourceMode(everyone.audio.output.player_audio), "everyone");
  const muted = applyPlayerSourceAction(everyone, { set_user_source_disabled: { user_id: alice.user_id!, source: "speaker", disabled: true } }, alice);
  assert.deepEqual(muted.audio.output.player_audio?.source_overrides?.[alice.user_id!], { chatbox_disabled: false, voice_disabled: true });
  assert.deepEqual(applyPlayerSourceAction(muted, { set_user_source_disabled: { user_id: alice.user_id!, source: "speaker", disabled: false } }, alice)
    .audio.output.player_audio?.source_overrides, {}, "restoring the default drops the entry");
});

test("players are ordered from nearest to farthest with unknown distances last", () => {
  const near = { ...alice, id: "near", index: 2 };
  const far = { ...bob, id: "far", index: 0 };
  const unknown = { ...alice, id: "unknown", index: 1 };
  const sorted = sortPlayerSpeakersByDistance([unknown, far, near], new Map([["near", 2.5], ["far", 18]]));
  assert.deepEqual(sorted.map(player => player.id), ["near", "far", "unknown"]);
});

test("current metadata wins without merging same-name accounts or separate tracks", () => {
  const historical = { ...alice, display_name: "Old Alice" };
  const rows = [{ source: "speaker", speaker: historical }] as Subtitle[];
  const speakers = collectPlayerSpeakers(rows, [alice, { ...alice, id: "vrc:a2" }], [bob.user_id], { [bob.user_id]: "Offline Bob" });
  assert.deepEqual(speakers.map(s => [s.id, s.display_name]), [["vrc:a", "Alice"], ["vrc:a2", "Alice"], [`saved-user:${bob.user_id}`, "Offline Bob"]]);
});

test("history keeps only saved accounts with a captured user ID", () => {
  const row = (speaker: Subtitle["speaker"]): Subtitle => ({ speaker, source: "speaker", translations: [], id: null,
    text: "record", language: null, started_at: null, ended_at: null, created_at: "2026-10-25T00:00:00Z" });
  const olderAlice = { ...alice, id: "vrc:a2", display_name: undefined };
  const unidentified = { id: "other:unidentified", index: 3, display_name: "Someone" };
  const unsaved = { ...bob, id: "vrc:b3", display_name: "Never configured" };
  const speakers = collectPlayerSpeakers(
    [row(alice), row(olderAlice), row(unsaved), row(unidentified)],
    [], [alice.user_id!], {},
  );
  assert.deepEqual(speakers.map(s => [s.id, s.display_name]), [["vrc:a", "Alice"]],
    "duplicate tracks collapse to one account, while records without a saved setting or a valid ID are dropped");
  assert.deepEqual(collectPlayerSpeakers([], [], ["usr_invalid"], {}), [],
    "selected keys that are not VRChat user IDs cannot create history rows");
});

test("a current track with pending metadata keeps its loaded historical name", () => {
  const rows = [{ source: "speaker", speaker: alice }] as Subtitle[];
  const [speaker] = collectPlayerSpeakers(rows, [{ ...alice, display_name: undefined }], [], {});
  assert.equal(speaker.display_name, "Alice");
});

test("player sources start off and are enabled per account", () => {
  const start = settings();
  const voice = applyPlayerSourceAction(start, { set_user_source_disabled: { user_id: alice.user_id!, source: "speaker", disabled: false } }, alice);
  assert.deepEqual(voice.audio.output.player_audio?.source_overrides, { [alice.user_id!]: { chatbox_disabled: true, voice_disabled: false } });
  assert.equal(voice.audio.output.player_audio?.selected_user_names?.[alice.user_id!], "Alice");
  const both = applyPlayerSourceAction(voice, { set_user_source_disabled: { user_id: alice.user_id!, source: "vrchat_chatbox", disabled: false } }, alice);
  assert.deepEqual(both.audio.output.player_audio?.source_overrides?.[alice.user_id!], { chatbox_disabled: false, voice_disabled: false });
  const muted = applyPlayerSourceAction(both, { set_user_source_disabled: { user_id: alice.user_id!, source: "speaker", disabled: true } }, alice);
  assert.deepEqual(muted.audio.output.player_audio?.source_overrides?.[alice.user_id!], { chatbox_disabled: false, voice_disabled: true });
  assert.deepEqual(applyPlayerSourceAction(muted, { set_user_source_disabled: { user_id: alice.user_id!, source: "vrchat_chatbox", disabled: true } }, alice)
    .audio.output.player_audio?.source_overrides, {});
  assert.deepEqual(applyPlayerSourceAction(start, { set_user_source_disabled: { user_id: alice.user_id!, source: "speaker", disabled: true } }, alice)
    .audio.output.player_audio?.source_overrides, {}, "muting an already muted source stores nothing");
  assert.throws(() => applyPlayerSourceAction(start, { set_user_source_disabled: { user_id: "unknown", source: "speaker", disabled: true } }));
});

test("track hiding is independent of explicitly selected languages", () => {
  const initial = { hiddenTracks: [alice.id], hiddenSourceLanguages: null };
  const manual = applyPlayerVisibilityAction(initial, { set_source_language_hidden: { language: "en", hidden: true } }, ["zh", "ja"]);
  assert.deepEqual(manual, { hiddenTracks: [alice.id], hiddenSourceLanguages: ["en"] });
  assert.deepEqual(applyPlayerVisibilityAction(manual, { show_all_tracks: null }, []), { hiddenTracks: [], hiddenSourceLanguages: ["en"] });
  const clear = applyPlayerVisibilityAction(manual, { clear_hidden_languages: null }, []);
  assert.deepEqual(clear, { hiddenTracks: [alice.id], hiddenSourceLanguages: [] });
  assert.deepEqual(applyPlayerVisibilityAction(clear, { set_follow_default: true }, ["zh"]).hiddenSourceLanguages, []);
});
