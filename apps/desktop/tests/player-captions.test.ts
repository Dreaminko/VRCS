import assert from "node:assert/strict";
import test from "node:test";
import type { TFunction } from "i18next";
import type { LiveTranscription } from "../src/capture/types.ts";
import type { PlayerAudioStatus } from "../src/core-client/types.ts";
import { playerCaptionMinimumHeight } from "../src/shell/compact-layout.ts";
import { latestCaptionsByTrack, selectPlayerPreviews } from "../src/shell/player-previews.ts";
import { playerAudioNotice } from "../src/capture/player-audio-notice.ts";

const preview = (index: number): LiveTranscription => ({ type: "partial", source: "speaker", utterance_id: `player-${index}-utterance`,
  text: `Player ${index}`, language: "en", speaker: { id: `player-${index}`, index, display_name: `Name ${index}` } });

test("parallel previews respect the display limit without counting the microphone or mutating upstream state", () => {
  const players = Array.from({ length: 40 }, (_, index) => preview(index));
  const microphone = { ...preview(99), source: "microphone" as const };
  assert.deepEqual(selectPlayerPreviews(players, [microphone], 4).map(item => item.speaker!.index), [0, 1, 2, 3, 99]);
  assert.equal(selectPlayerPreviews(players, [microphone], 0).length, 41);
  assert.equal(players.length, 40);
  assert.deepEqual(selectPlayerPreviews([...players.slice(1), { ...players[0], text: "new delta" }], [], 4).map(item => item.speaker!.index), [0, 1, 2, 3]);
  assert.deepEqual(selectPlayerPreviews(players, [microphone]), [microphone]);
});

test("live previews replace only their own track while other players retain their latest completed caption", () => {
  const first = preview(0);
  const second = { ...preview(1), speaker: { ...preview(1).speaker!, display_name: first.speaker!.display_name } };
  const latestSecond = { ...second, text: "Completed second player" };
  const currentFirst = { ...first, text: "First player still speaking" };
  const microphone = { ...preview(0), source: "microphone" as const, text: "My microphone" };
  const captions = [second, first, latestSecond, currentFirst, microphone];
  assert.deepEqual(latestCaptionsByTrack(captions), [currentFirst, latestSecond, microphone]);
  assert.equal(captions.length, 5);
});

test("Chatbox messages keep their recent history instead of replacing a speaker track", () => {
  const player = preview(0);
  const latestPlayer = { ...player, text: "Player keeps speaking" };
  const first = { source: "chatbox", text: "First message" };
  const second = { source: "chatbox", text: "Second message" };
  const third = { source: "chatbox", text: "Third message" };
  const captions = [player, first, second, latestPlayer, third];
  assert.deepEqual(latestCaptionsByTrack(captions), [latestPlayer, first, second, third]);
  assert.deepEqual(latestCaptionsByTrack(captions, 2), [latestPlayer, second, third]);
  assert.equal(captions.length, 5);
});

test("VR captions keep waiting and failure notices without duplicating key acquisition progress", () => {
  const t = ((key: string) => key) as TFunction;
  const status: PlayerAudioStatus = { phase: "waiting_game", captured_packets: 0, decrypted_packets: 0,
    pcm_frames: 0, active_players: 0, dropped_frames: 0, decode_errors: 0, detail: null };
  for (const phase of ["waiting_game", "waiting_world", "waiting_traffic", "failed"] as const) {
    const notice = playerAudioNotice({ ...status, phase }, true, t)!;
    assert.equal(notice.key, phase);
    assert.equal(notice.duration_ms, null);
    assert.match(notice.text, new RegExp(phase));
  }
  assert.match(playerAudioNotice({ ...status, phase: "failed", detail: "Scan timed out" }, true, t)!.text, /Scan timed out/);
  const ready = { ...status, active_players: 1, phase: "streaming" as const, key_scan: { stage: "verified" as const, elapsed_ms: 1234, scan_elapsed_ms: 900, scanned_bytes: 0, total_bytes: 0, candidates_tested: 0, workers: 1 } };
  for (const phase of ["waiting_key", "acquiring_key", "retrying_key", "streaming"] as const) {
    assert.equal(playerAudioNotice({ ...ready, phase }, true, t), null);
  }
  assert.equal(playerAudioNotice({ ...ready, key_scan: { ...ready.key_scan, stage: "cached" } }, true, t), null);
  assert.equal(playerAudioNotice(ready, false, t), null);
  assert.equal(playerAudioNotice({ ...status, phase: "stopped" }, true, t), null);
});


test("vertical caption height reserves names, both languages and all row gaps", () => {
  const four = playerCaptionMinimumHeight(4, "medium");
  assert.ok(four >= 340 && four < 400);
  assert.ok(playerCaptionMinimumHeight(4, "large") > four);
  assert.ok(playerCaptionMinimumHeight(2, "medium") < four);
});
