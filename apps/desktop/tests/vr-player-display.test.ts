import assert from "node:assert/strict";
import test from "node:test";
import { createPlayerDisplayExecutor, createPlayerDisplayPublisher, createPlayerDisplayReceiver, type PlayerDisplayView } from "../src/vr-controls/player-display-protocol.ts";
import { createSettingsAutosave } from "../src/settings/settings-autosave.ts";
import type { Settings } from "../src/settings/types.ts";
import type { PlayerHistoryPreferences } from "../src/subtitles/player-history.ts";

test("personnel requests reject stale owners/revisions and execute each request once", async () => {
  let calls = 0;
  const results: Array<string | null> = [];
  const receive = createPlayerDisplayReceiver({ epoch: 3, revision: () => 5,
    execute: async () => { calls++; }, publish: async () => {},
    finish: async (_request, error) => { results.push(error); }, active: () => true });
  const request = { owner_epoch: 3, request_id: 1, base_revision: 5, source: "wrist_menu" as const, action: { show_all_tracks: null } };
  await receive({ ...request, owner_epoch: 2 });
  assert.equal(calls, 0);
  await receive(request);
  await receive(request);
  assert.equal(calls, 1);
  await receive({ ...request, request_id: 2, base_revision: 4 });
  await receive({ ...request, request_id: 3, base_revision: 6 });
  assert.equal(calls, 1);
  assert.match(results[1]!, /changed/);
  assert.match(results[2]!, /changed/);
});

test("failed saves publish the confirmed view before clearing pending and a released owner sends no result", async () => {
  const order: string[] = [];
  let active = true;
  const receive = createPlayerDisplayReceiver({ epoch: 1, revision: () => 1,
    execute: async () => { throw new Error("Save failed"); },
    publish: async () => { order.push("publish"); },
    finish: async (_request, error) => { order.push(error!); }, active: () => active });
  const request = { owner_epoch: 1, request_id: 1, base_revision: 1, source: "wrist_menu" as const, action: { clear_selection: null } };
  await receive(request);
  assert.deepEqual(order, ["publish", "Save failed"]);
  active = false;
  await receive({ ...request, request_id: 2 });
  assert.equal(order.length, 2);
});

test("a publication failure still clears pending with an error", async () => {
  const errors: Array<string | null> = [];
  const receive = createPlayerDisplayReceiver({ epoch: 1, revision: () => 1, active: () => true,
    execute: async () => {}, publish: async () => { throw new Error("Snapshot unavailable"); },
    finish: async (_request, error) => { errors.push(error); } });
  await receive({ owner_epoch: 1, request_id: 1, base_revision: 1, source: "wrist_menu", action: { show_all_tracks: null } });
  assert.deepEqual(errors, ["Snapshot unavailable"]);
});

test("releasing the owner during a save suppresses its old publication and result", async () => {
  let active = true;
  const order: string[] = [];
  const receive = createPlayerDisplayReceiver({ epoch: 1, revision: () => 1, active: () => active,
    execute: async () => { active = false; }, publish: async () => { order.push("publish"); },
    finish: async () => { order.push("finish"); } });
  await receive({ owner_epoch: 1, request_id: 1, base_revision: 1, source: "wrist_menu", action: { show_all_tracks: null } });
  assert.deepEqual(order, []);
});

const emptyView = (): PlayerDisplayView => ({
  available: true, processing_available: true, player_mode: true, selection_mode: "selected", selected_count: 0,
  players: [], languages: [], follow_default: true, default_languages: [], labels: {}, busy: false, error: null,
});

test("microphone controls keep their capture generation and do not alter player selection or visibility", async () => {
  const actions: unknown[] = [];
  const execute = createPlayerDisplayExecutor({
    getState: () => ({ available: true, processingAvailable: false, speakers: [], current: [], defaultLanguages: [], languages: [] }),
    updateSettings: async () => { throw new Error("Unexpected player settings change"); },
    updatePreferences: () => { throw new Error("Unexpected visibility change"); },
    updateMicrophone: async action => { actions.push(action); },
  });
  await execute({ set_microphone_filter: true });
  const selection = { set_microphone_speaker: { generation: "mic-session", revision: 4, id: "speaker-2", decision: "block" as const } };
  await execute(selection);
  assert.deepEqual(actions, [{ set_microphone_filter: true }, selection]);
});

test("snapshot deduplication waits for its in-flight update and does not lose a reverted state", async () => {
  let view = emptyView();
  const sent: Array<{ revision: number; selected: number }> = [];
  const complete: Array<() => void> = [];
  const publisher = createPlayerDisplayPublisher({ getView: () => view,
    update: (revision, next) => {
      sent.push({ revision, selected: next.selected_count });
      return new Promise<void>(resolve => complete.push(resolve));
    } });
  const first = publisher.publish();
  complete.shift()!();
  await first;
  await publisher.publish();
  view = { ...view, selected_count: 1 };
  const changed = publisher.publish();
  let duplicateFinished = false;
  const duplicate = publisher.publish().then(() => { duplicateFinished = true; });
  await Promise.resolve();
  assert.equal(duplicateFinished, false);
  view = { ...view, selected_count: 0 };
  const reverted = publisher.publish();
  assert.deepEqual(sent, [{ revision: 1, selected: 0 }, { revision: 2, selected: 1 }, { revision: 3, selected: 0 }]);
  assert.equal(publisher.revision(), 3);
  complete.splice(0).forEach(resolve => resolve());
  await Promise.all([changed, duplicate, reverted]);
});

const alice = { id: "vrc:a", index: 0, user_id: "usr_00000000-0000-4000-8000-000000000001", display_name: "Alice" };
const bob = { id: "vrc:b", index: 1, user_id: "usr_00000000-0000-4000-8000-000000000002", display_name: "Bob" };
const initialSettings = () => ({ audio: { output: { mode: "vrchat_players", player_audio: {
  selected_tracks: [], selected_user_names: {}, max_players: 3,
} } }, osc: { enabled: false } }) as unknown as Settings;

test("display actions validate live identities, actual tracks, language candidates and modes", async () => {
  let settings = initialSettings();
  let preferences: PlayerHistoryPreferences = { hiddenTracks: [], hiddenSourceLanguages: null };
  let available = true;
  let processingAvailable = true;
  const execute = createPlayerDisplayExecutor({
    getState: () => ({ available, processingAvailable, speakers: [alice, { id: "vrc:unknown", index: 2 },
      { id: `saved-user:${bob.user_id}`, user_id: bob.user_id, index: 0 }], current: [alice],
      defaultLanguages: ["ja"], languages: ["ja", "en"] }),
    updateSettings: async apply => (settings = apply(settings)),
    updatePreferences: apply => { preferences = apply(preferences); },
  });
  await assert.rejects(execute({ set_user_selected: { user_id: "unknown", selected: true } }), /identity/);
  await assert.rejects(execute({ set_track_hidden: { track_id: `saved-user:${bob.user_id}`, hidden: true } }), /track/i);
  await assert.rejects(execute({ set_track_hidden: { track_id: "missing", hidden: true } }), /track/i);
  await assert.rejects(execute({ set_source_language_hidden: { language: "fr", hidden: true } }), /language/i);
  await execute({ set_source_language_hidden: { language: "en-US", hidden: true } });
  assert.deepEqual(preferences.hiddenSourceLanguages, ["en"]);
  processingAvailable = false;
  await assert.rejects(execute({ set_processing_mode: "all" }), /unavailable/);
  await execute({ set_track_hidden: { track_id: "vrc:unknown", hidden: true } });
  assert.deepEqual(preferences.hiddenTracks, ["vrc:unknown"]);
  processingAvailable = true;
  settings = { ...settings, audio: { ...settings.audio, output: { ...settings.audio.output, mode: "system" } } } as Settings;
  await assert.rejects(execute({ set_user_selected: { user_id: alice.user_id, selected: true } }), /unavailable/);
  available = false;
  await assert.rejects(execute({ show_all_tracks: null }), /unavailable/);
});

test("VR selection shares the existing settings queue and reapplies after a conflict", async () => {
  let confirmed = initialSettings();
  let conflict = true;
  const autosave = createSettingsAutosave({ getCurrent: () => confirmed,
    persist: async next => {
      if (conflict) { conflict = false; throw { status: 409 }; }
      confirmed = next;
      return next;
    },
    reload: async () => {
      confirmed = { ...confirmed, osc: { ...confirmed.osc, enabled: true } };
      return confirmed;
    }, onOptimistic: () => {}, onCommit: () => {}, onError: () => {},
  });
  const execute = createPlayerDisplayExecutor({
    getState: () => ({ available: true, processingAvailable: true, speakers: [alice, bob], current: [alice, bob],
      defaultLanguages: [], languages: [] }),
    updateSettings: autosave.update, updatePreferences: () => {},
  });
  const vrSave = execute({ set_user_selected: { user_id: alice.user_id, selected: true } });
  const desktopSave = autosave.update(current => ({ ...current, audio: { ...current.audio, output: { ...current.audio.output,
    player_audio: { ...current.audio.output.player_audio!, selected_tracks: [...current.audio.output.player_audio!.selected_tracks!, bob.user_id] },
  } } }));
  await Promise.all([vrSave, desktopSave]);
  assert.deepEqual(confirmed.audio.output.player_audio?.selected_tracks, [alice.user_id, bob.user_id]);
  assert.equal(confirmed.osc.enabled, true);
  assert.equal(confirmed.audio.output.player_audio?.max_players, 3);
});

test("a processing save cannot report success when Core returns an unapplied target", async () => {
  const settings = initialSettings();
  const execute = createPlayerDisplayExecutor({
    getState: () => ({ available: true, processingAvailable: true, speakers: [alice], current: [alice],
      defaultLanguages: [], languages: [] }),
    updateSettings: async apply => { apply(settings); return settings; }, updatePreferences: () => {},
  });
  await assert.rejects(execute({ set_user_selected: { user_id: alice.user_id, selected: true } }), /not applied/);
});

test("show all tracks also clears hidden tracks outside the loaded conversation", async () => {
  let preferences: PlayerHistoryPreferences = { hiddenTracks: ["unloaded-track"], hiddenSourceLanguages: ["ja"] };
  const execute = createPlayerDisplayExecutor({
    getState: () => ({ available: true, processingAvailable: false, speakers: [], current: [],
      defaultLanguages: [], languages: ["ja"] }),
    updateSettings: async apply => apply(initialSettings()), updatePreferences: apply => { preferences = apply(preferences); },
  });
  await execute({ show_all_tracks: null });
  assert.deepEqual(preferences, { hiddenTracks: [], hiddenSourceLanguages: ["ja"] });
});
