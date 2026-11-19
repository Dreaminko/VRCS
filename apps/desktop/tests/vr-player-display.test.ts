import assert from "node:assert/strict";
import test from "node:test";
import { createPlayerDisplayExecutor, createPlayerDisplayPublisher, createPlayerDisplayReceiver, type PlayerDisplayView } from "../src/vr-controls/player-display-protocol.ts";
import { sourceMode } from "../src/subtitles/player-controls.ts";
import { createSettingsAutosave } from "../src/settings/settings-autosave.ts";
import type { Settings } from "../src/settings/types.ts";
import type { PlayerHistoryPreferences } from "../src/subtitles/player-history.ts";

test("personnel requests reject stale owners, accept queued revisions, and execute each request once", async () => {
  let calls = 0;
  const results: Array<string | null> = [];
  const receive = createPlayerDisplayReceiver({ epoch: 3,
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
  assert.equal(calls, 3);
  assert.deepEqual(results, [null, null, null]);
});

test("failed saves publish the confirmed view before clearing pending and a released owner sends no result", async () => {
  const order: string[] = [];
  let active = true;
  const receive = createPlayerDisplayReceiver({ epoch: 1,
    execute: async () => { throw new Error("Save failed"); },
    publish: async () => { order.push("publish"); },
    finish: async (_request, error) => { order.push(error!); }, active: () => active });
  const request = { owner_epoch: 1, request_id: 1, base_revision: 1, source: "wrist_menu" as const, action: { show_all_tracks: null } };
  await receive(request);
  assert.deepEqual(order, ["publish", "Save failed"]);
  active = false;
  await receive({ ...request, request_id: 2 });
  assert.equal(order.length, 2);
});

test("a publication failure still clears pending with an error", async () => {
  const errors: Array<string | null> = [];
  const receive = createPlayerDisplayReceiver({ epoch: 1, active: () => true,
    execute: async () => {}, publish: async () => { throw new Error("Snapshot unavailable"); },
    finish: async (_request, error) => { errors.push(error); } });
  await receive({ owner_epoch: 1, request_id: 1, base_revision: 1, source: "wrist_menu", action: { show_all_tracks: null } });
  assert.deepEqual(errors, ["Snapshot unavailable"]);
});

test("releasing the owner during a save suppresses its old publication and result", async () => {
  let active = true;
  const order: string[] = [];
  const receive = createPlayerDisplayReceiver({ epoch: 1, active: () => active,
    execute: async () => { active = false; }, publish: async () => { order.push("publish"); },
    finish: async () => { order.push("finish"); } });
  await receive({ owner_epoch: 1, request_id: 1, base_revision: 1, source: "wrist_menu", action: { show_all_tracks: null } });
  assert.deepEqual(order, []);
});

const emptyView = (): PlayerDisplayView => ({
  available: true, processing_available: true, player_mode: true, source_mode: "allowlist",
  players: [], languages: [], follow_default: true, default_languages: [], labels: {}, busy: false, error: null,
});

test("proximity controls update the shared player audio settings and reject invalid ranges", async () => {
  let settings = initialSettings();
  const execute = createPlayerDisplayExecutor({
    getState: () => ({ available: true, processingAvailable: true, speakers: [], current: [], hiddenTracks: [], defaultLanguages: [], languages: [] }),
    updateSettings: async apply => (settings = apply(settings)),
    updatePreferences: () => { throw new Error("Unexpected visibility change"); },
  });
  await execute({ set_proximity_radius: 4.46 });
  assert.deepEqual(settings.audio.output.player_audio?.proximity, { enabled: true, radius_m: 4.5 });
  await execute({ set_proximity_radius: 0 });
  assert.deepEqual(settings.audio.output.player_audio?.proximity, { enabled: false, radius_m: 0 });
  await execute({ set_proximity_radius: 2 });
  assert.deepEqual(settings.audio.output.player_audio?.proximity, { enabled: true, radius_m: 2 });
  await assert.rejects(execute({ set_proximity_radius: -0.1 }), /invalid/i);
  await assert.rejects(execute({ set_proximity_radius: 10.1 }), /invalid/i);
  await assert.rejects(execute({ set_proximity_radius: Number.NaN }), /invalid/i);
  settings = { ...settings, audio: { ...settings.audio, output: { ...settings.audio.output, mode: "system" } } } as Settings;
  await assert.rejects(execute({ set_proximity_radius: 5 }), /unavailable/i);
});

test("proximity controls repair a stored radius whose enabled flag disagrees with it", async () => {
  const legacy = { ...initialSettings(), audio: { output: { mode: "vrchat_players", player_audio: {
    selected_user_names: {}, max_players: 3, proximity: { enabled: false, radius_m: 5 },
  } } } } as unknown as Settings;
  let settings = legacy;
  const execute = createPlayerDisplayExecutor({
    getState: () => ({ available: true, processingAvailable: true, speakers: [], current: [], hiddenTracks: [], defaultLanguages: [], languages: [] }),
    updateSettings: async apply => (settings = apply(settings)),
    updatePreferences: () => {},
  });
  await execute({ set_proximity_radius: 5 });
  assert.deepEqual(settings.audio.output.player_audio?.proximity, { enabled: true, radius_m: 5 });
});

test("proximity controls are unavailable when the saved settings have no proximity block", async () => {
  const initial = initialSettings();
  const settings = { ...initial, audio: { ...initial.audio, output: { ...initial.audio.output,
    player_audio: { ...initial.audio.output.player_audio!, proximity: undefined } } } } as Settings;
  const execute = createPlayerDisplayExecutor({
    getState: () => ({ available: true, processingAvailable: true, speakers: [], current: [], hiddenTracks: [], defaultLanguages: [], languages: [] }),
    updateSettings: async apply => apply(settings),
    updatePreferences: () => {},
  });
  await assert.rejects(execute({ set_proximity_radius: 5 }), /unavailable/i);
});

test("view priority requires published player settings, bounds the angle and preserves confirmed saves", async () => {
  let settings = initialSettings();
  let applySave = true;
  let failSave = false;
  const execute = createPlayerDisplayExecutor({
    getState: () => ({ available: true, processingAvailable: true, speakers: [], current: [], hiddenTracks: [], defaultLanguages: [], languages: [] }),
    updateSettings: async apply => {
      const next = apply(settings);
      if (failSave) throw new Error("Save failed");
      return applySave ? (settings = next) : settings;
    },
    updatePreferences: () => { throw new Error("Unexpected visibility change"); },
  });
  const setView = (enabled: boolean, horizontal_fov_deg: number) => execute({ set_view_priority: { enabled, horizontal_fov_deg } });
  await assert.rejects(setView(true, 120), /unavailable/i);
  settings = { ...settings, audio: { ...settings.audio, output: { ...settings.audio.output,
    player_audio: { ...settings.audio.output.player_audio!, view_priority: { enabled: false, horizontal_fov_deg: 120 } },
  } } };
  await setView(true, 30);
  assert.deepEqual(settings.audio.output.player_audio?.view_priority, { enabled: true, horizontal_fov_deg: 30 });
  await setView(false, 180);
  assert.deepEqual(settings.audio.output.player_audio?.view_priority, { enabled: false, horizontal_fov_deg: 180 });
  for (const angle of [29, 181, Number.NaN]) await assert.rejects(setView(true, angle), /invalid/i);
  const confirmed = settings;
  failSave = true;
  await assert.rejects(setView(true, 90), /Save failed/);
  assert.equal(settings, confirmed);
  failSave = false;
  applySave = false;
  await assert.rejects(setView(true, 90), /not applied/i);
  settings = { ...settings, audio: { ...settings.audio, output: { ...settings.audio.output, mode: "system" } } } as Settings;
  await assert.rejects(setView(true, 90), /unavailable/i);
});

test("source mode switches the shared player audio policy from VR", async () => {
  let settings = initialSettings();
  const execute = createPlayerDisplayExecutor({
    getState: () => ({ available: true, processingAvailable: true, speakers: [], current: [], hiddenTracks: [], defaultLanguages: [], languages: [] }),
    updateSettings: async apply => (settings = apply(settings)),
    updatePreferences: () => { throw new Error("Unexpected visibility change"); },
  });
  await execute({ set_source_mode: "everyone" });
  assert.equal(sourceMode(settings.audio.output.player_audio), "everyone");
  await execute({ set_source_mode: "allowlist" });
  assert.equal(sourceMode(settings.audio.output.player_audio), "allowlist");
  await assert.rejects(execute({ set_source_mode: "system" as never }), /invalid/i);
  const unchanged = createPlayerDisplayExecutor({
    getState: () => ({ available: true, processingAvailable: true, speakers: [], current: [], hiddenTracks: [], defaultLanguages: [], languages: [] }),
    updateSettings: async () => settings, updatePreferences: () => {},
  });
  await assert.rejects(unchanged({ set_source_mode: "everyone" }), /not applied/);
});

test("snapshot deduplication waits for its in-flight update and does not lose a reverted state", async () => {
  let view = emptyView();
  const sent: Array<{ revision: number; follow: boolean }> = [];
  const complete: Array<() => void> = [];
  const publisher = createPlayerDisplayPublisher({ getView: () => view,
    update: (revision, next) => {
      sent.push({ revision, follow: next.follow_default });
      return new Promise<void>(resolve => complete.push(resolve));
    } });
  const first = publisher.publish();
  complete.shift()!();
  await first;
  await publisher.publish();
  view = { ...view, follow_default: false };
  const changed = publisher.publish();
  let duplicateFinished = false;
  const duplicate = publisher.publish().then(() => { duplicateFinished = true; });
  await Promise.resolve();
  assert.equal(duplicateFinished, false);
  view = { ...view, follow_default: true };
  const reverted = publisher.publish();
  assert.deepEqual(sent, [{ revision: 1, follow: true }, { revision: 2, follow: false }, { revision: 3, follow: true }]);
  assert.equal(publisher.revision(), 3);
  complete.splice(0).forEach(resolve => resolve());
  await Promise.all([changed, duplicate, reverted]);
});

const alice = { id: "vrc:a", index: 0, user_id: "usr_00000000-0000-4000-8000-000000000001", display_name: "Alice" };
const bob = { id: "vrc:b", index: 1, user_id: "usr_00000000-0000-4000-8000-000000000002", display_name: "Bob" };
const initialSettings = () => ({ audio: { output: { mode: "vrchat_players", player_audio: {
  selected_user_names: {}, max_players: 3, proximity: { enabled: false, radius_m: 10 },
} } }, osc: { enabled: false } }) as unknown as Settings;

test("display actions validate live identities, actual tracks and language candidates", async () => {
  let settings = initialSettings();
  let preferences: PlayerHistoryPreferences = { hiddenTracks: [], hiddenSourceLanguages: null };
  let available = true;
  const execute = createPlayerDisplayExecutor({
    getState: () => ({ available, processingAvailable: true, speakers: [alice, { id: "vrc:unknown", index: 2 },
      { id: `saved-user:${bob.user_id}`, user_id: bob.user_id, index: 0 }], current: [alice],
      hiddenTracks: preferences.hiddenTracks, defaultLanguages: ["ja"], languages: ["ja", "en"] }),
    updateSettings: async apply => (settings = apply(settings)),
    updatePreferences: apply => { preferences = apply(preferences); },
  });
  await assert.rejects(execute({ set_user_source_disabled: { user_id: "unknown", source: "speaker", disabled: true } }), /identity/);
  await assert.rejects(execute({ set_track_hidden: { track_id: `saved-user:${bob.user_id}`, hidden: true } }), /track/i);
  await assert.rejects(execute({ set_track_hidden: { track_id: "missing", hidden: true } }), /track/i);
  await assert.rejects(execute({ set_source_language_hidden: { language: "fr", hidden: true } }), /language/i);
  await execute({ set_source_language_hidden: { language: "en-US", hidden: true } });
  assert.deepEqual(preferences.hiddenSourceLanguages, ["en"]);
  await execute({ set_track_hidden: { track_id: "vrc:unknown", hidden: true } });
  assert.deepEqual(preferences.hiddenTracks, ["vrc:unknown"]);
  await execute({ set_track_hidden: { track_id: alice.id, hidden: true } });
  assert.equal(Boolean(settings.audio.output.player_audio?.source_overrides?.[alice.user_id!]?.voice_disabled ?? true), true, "hiding leaves voice muted");
  await assert.rejects(execute({ set_user_source_disabled: { user_id: alice.user_id!, source: "speaker", disabled: false } }), /unhide/i);
  await execute({ set_track_hidden: { track_id: alice.id, hidden: false } });
  assert.equal(Boolean(settings.audio.output.player_audio?.source_overrides?.[alice.user_id!]?.voice_disabled ?? true), true, "unhiding alone does not enable voice");
  await execute({ set_user_source_disabled: { user_id: alice.user_id!, source: "speaker", disabled: false } });
  assert.equal(settings.audio.output.player_audio?.source_overrides?.[alice.user_id!]?.voice_disabled, false);
  await execute({ set_user_source_disabled: { user_id: alice.user_id!, source: "speaker", disabled: true } });
  assert.equal(Boolean(settings.audio.output.player_audio?.source_overrides?.[alice.user_id!]?.voice_disabled ?? true), true);
  available = false;
  await assert.rejects(execute({ show_all_tracks: null }), /unavailable/);
});

test("VR source settings share the existing queue and reapply after a conflict", async () => {
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
      hiddenTracks: [], defaultLanguages: [], languages: [] }),
    updateSettings: autosave.update, updatePreferences: () => {},
  });
  const vrSave = execute({ set_user_source_disabled: { user_id: alice.user_id!, source: "vrchat_chatbox", disabled: false } });
  const desktopSave = autosave.update(current => ({ ...current, audio: { ...current.audio, output: { ...current.audio.output,
    player_audio: { ...current.audio.output.player_audio!, max_players: 5 },
  } } }));
  await Promise.all([vrSave, desktopSave]);
  assert.equal(confirmed.audio.output.player_audio?.source_overrides?.[alice.user_id!]?.chatbox_disabled, false);
  assert.equal(confirmed.osc.enabled, true);
  assert.equal(confirmed.audio.output.player_audio?.max_players, 5);
});

test("a source save cannot report success when Core returns an unapplied target", async () => {
  const settings = initialSettings();
  const execute = createPlayerDisplayExecutor({
    getState: () => ({ available: true, processingAvailable: true, speakers: [alice], current: [alice],
      hiddenTracks: [], defaultLanguages: [], languages: [] }),
    updateSettings: async apply => { apply(settings); return settings; }, updatePreferences: () => {},
  });
  await assert.rejects(execute({ set_user_source_disabled: { user_id: alice.user_id!, source: "speaker", disabled: false } }), /not applied/);
});

test("show all tracks also clears hidden tracks outside the loaded conversation", async () => {
  let preferences: PlayerHistoryPreferences = { hiddenTracks: ["unloaded-track"], hiddenSourceLanguages: ["ja"] };
  const execute = createPlayerDisplayExecutor({
    getState: () => ({ available: true, processingAvailable: false, speakers: [], current: [],
      hiddenTracks: [], defaultLanguages: [], languages: ["ja"] }),
    updateSettings: async apply => apply(initialSettings()), updatePreferences: apply => { preferences = apply(preferences); },
  });
  await execute({ show_all_tracks: null });
  assert.deepEqual(preferences, { hiddenTracks: [], hiddenSourceLanguages: ["ja"] });
});
