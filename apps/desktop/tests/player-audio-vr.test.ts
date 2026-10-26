import assert from "node:assert/strict";
import test from "node:test";
import type { TFunction } from "i18next";
import type { PlayerAudioStatus } from "../src/core-client/types.ts";
import { playerAudioPrompt, playerAudioProgress } from "../src/capture/player-audio-vr.ts";

const t = ((key: string, values?: { seconds?: number }) => `${key}${values?.seconds === undefined ? "" : `:${values.seconds}`}`) as TFunction;
const status: PlayerAudioStatus = { phase: "waiting_game", captured_packets: 0, decrypted_packets: 0, pcm_frames: 0,
  active_players: 0, dropped_frames: 0, decode_errors: 0, detail: null };

test("VR consent contains the full desktop explanation and shares its reading deadline", () => {
  const notice = { requestId: 12, openedAt: 100 };
  const prompt = playerAudioPrompt(notice, 2_999, t)!;
  assert.equal(prompt.request_id, 12);
  assert.equal(prompt.remaining_seconds, 1);
  assert.equal(prompt.sections.length, 5);
  assert.equal(prompt.sections[0].paragraphs.length, 3);
  assert.equal(prompt.sections[3].paragraphs.length, 2);
  assert.equal(playerAudioPrompt(notice, 3_100, t)!.remaining_seconds, 0);
  assert.equal(playerAudioPrompt(null, 4_000, t), null);
});

test("VR preparation uses the OCR stages for prerequisites and the actual player audio phases", () => {
  const build = (phase: PlayerAudioStatus["phase"]) => playerAudioProgress(null, { ...status, phase }, true, t)!;
  assert.deepEqual(playerAudioProgress({ stage: "privileges", detail: null }, null, false, t)!.stages,
    ["active", "pending", "pending", "pending"]);
  assert.deepEqual(build("waiting_game").stages, ["complete", "active", "pending", "pending"]);
  assert.deepEqual(build("waiting_traffic").stages, ["complete", "complete", "active", "pending"]);
  assert.deepEqual(build("acquiring_key").stages, ["complete", "complete", "active", "pending"]);
  assert.deepEqual(build("retrying_key").stages, ["complete", "complete", "active", "pending"]);
  assert.deepEqual(build("failed").stages, ["complete", "complete", "failed", "pending"]);
  assert.equal(playerAudioProgress(null, status, false, t), null);
  assert.equal(playerAudioProgress(null, { ...status, phase: "stopped" }, true, t), null);
  assert.equal(playerAudioProgress(null, { ...status, phase: "streaming" }, true, t), null);
});

test("VR key acquisition reports measured scan progress without treating paging or retry as success", () => {
  const key_scan = { stage: "scanning" as const, scanned_bytes: 120, total_bytes: 400, elapsed_ms: 2_500,
    scan_elapsed_ms: 1_000, candidates_tested: 0, workers: 2 };
  const scan = playerAudioProgress(null, { ...status, phase: "acquiring_key", key_scan }, true, t)!;
  assert.deepEqual(scan.translation_fraction, [120, 400]);
  assert.equal(scan.elapsed_seconds, 2);
  assert.match(scan.status, /scanning/);
  assert.equal(playerAudioProgress(null, { ...status, phase: "acquiring_key", key_scan: { ...key_scan, stage: "paging" } }, true, t)!.translation_fraction, null);
  assert.equal(playerAudioProgress(null, { ...status, phase: "retrying_key", key_scan }, true, t)!.translation_fraction, null);
});

test("blocked prerequisites and failures stay explicit and take priority over stale capture status", () => {
  const view = playerAudioProgress({ stage: "blocked", detail: "Restart Windows" }, { ...status, phase: "streaming" }, true, t)!;
  assert.equal(view.stages[0], "failed");
  assert.match(view.detail, /Restart Windows/);
  assert.match(view.detail, /desktop/);
  assert.match(playerAudioProgress({ stage: "failed", detail: "UAC cancelled" }, null, false, t)!.detail, /UAC cancelled/);
});
