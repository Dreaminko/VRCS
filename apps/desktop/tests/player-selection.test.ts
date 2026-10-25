import assert from "node:assert/strict";
import test from "node:test";
import { playerUserId, withCurrentPlayerNames } from "../src/capture/player-selection.ts";
import { createSettingsAutosave } from "../src/settings/settings-autosave.ts";
import type { Settings } from "../src/settings/types.ts";

const user = "usr_00000000-0000-4000-8000-000000000007";
const other = "usr_00000000-0000-4000-8000-000000000008";
const speaker = { id:"vrc:new-world:new-track", index:7, user_id:user, display_name:"Current name" };
function settings(): Settings {
  const audio: Settings["audio"] = { sample_rate:16000, microphone:{mode:"disabled",device_id:null,trigger_threshold_dbfs:-45},
    output: { mode:"vrchat_players", device_id:null, trigger_threshold_dbfs:-45, player_audio: {
    pagefile_drive:"C:", opus_library_path:"", max_players:0, selected_tracks:[user, other],
    selected_user_names:{[user]:"Previous name",[other]:"Offline player"},
  } } };
  return { audio } as Settings;
}

test("a saved account is renamed on its new track while offline selections and other settings remain intact", () => {
  const original = settings();
  const updated = withCurrentPlayerNames(original, [speaker]);
  assert.deepEqual(updated.audio.output.player_audio?.selected_user_names, {[user]:"Current name",[other]:"Offline player"});
  assert.equal(updated.audio.output.player_audio?.selected_tracks, original.audio.output.player_audio?.selected_tracks);
  assert.equal(updated.audio.microphone, original.audio.microphone);
  assert.equal(original.audio.output.player_audio?.selected_user_names?.[user], "Previous name");
  assert.equal(withCurrentPlayerNames(updated, [speaker]), updated, "an unchanged name must not schedule another file write");
  const restored = JSON.parse(JSON.stringify(updated)) as Settings;
  assert.equal(withCurrentPlayerNames(restored, [{...speaker,id:"vrc:another-session",index:99}]), restored);
});

test("only a confirmed selected user ID can change a saved label", () => {
  const original = settings();
  for (const user_id of [undefined, null, "Previous name", "usr_invalid", "usr_00000000-0000-4000-8000-000000000009"]) {
    assert.equal(withCurrentPlayerNames(original,[{...speaker,user_id}]), original);
  }
  for (const display_name of [null, "", "bad\nname", "x".repeat(129)]) {
    assert.equal(withCurrentPlayerNames(original,[{...speaker,display_name}]), original);
  }
  assert.equal(playerUserId({...speaker,user_id:null}), null);
  assert.equal(playerUserId(speaker), user);
  original.audio.output.player_audio!.selected_tracks = null;
  assert.equal(withCurrentPlayerNames(original,[speaker]), original);
});

test("a queued automatic name refresh cannot restore a removed player or overwrite a concurrent setting", async () => {
  let current = settings();
  let release!: () => void;
  const gate = new Promise<void>(resolve => { release = resolve; });
  const writes: Settings[] = [];
  const save = createSettingsAutosave<Settings>({
    getCurrent:()=>current,
    persist:async next=>{ if (!writes.length) await gate; writes.push(next); current=next; return next; },
    onOptimistic:()=>{},onCommit:()=>{},onError:()=>{},
  });
  const change = save.update(value=>({...value,audio:{...value.audio,output:{...value.audio.output,
    trigger_threshold_dbfs:-35,player_audio:{...value.audio.output.player_audio!,selected_tracks:[other],selected_user_names:{[other]:"Offline player"}},
  }}}));
  const refresh = save.update(value=>withCurrentPlayerNames(value,[speaker]));
  release();
  await Promise.all([change,refresh]);
  assert.equal(writes.length,1);
  assert.equal(current.audio.output.trigger_threshold_dbfs,-35);
  assert.deepEqual(current.audio.output.player_audio?.selected_tracks,[other]);
  assert.deepEqual(current.audio.output.player_audio?.selected_user_names,{[other]:"Offline player"});
});
