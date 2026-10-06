import assert from "node:assert/strict";
import test from "node:test";
import { applyDesktopOcrStatus, desktopOcrCopyText, type DesktopOcrStatus } from "../src/ocr/status.ts";

const status: DesktopOcrStatus = {
  scan_id: 3, revision: 4, state: "translating", error: null, shortcut_error: null, timed_out: false,
  blocks: [{ source: { id: 1, text: "Hello", confidence: 0.9, polygon: [] }, translations: [
    { target_language: "zh-Hans", text: "translated", error_code: null },
    { target_language: "ja", text: null, error_code: "translation.timeout" },
  ] }],
};

test("OCR snapshots cannot replace newer scan or revision events", () => {
  assert.equal(applyDesktopOcrStatus(status, { ...status, scan_id: 2, revision: 10 }), status);
  assert.equal(applyDesktopOcrStatus(status, { ...status, revision: 3 }), status);
  assert.equal(applyDesktopOcrStatus(status, { ...status, revision: 4, state: "recognizing" }), status);
  const completed = { ...status, revision: 5, state: "complete" as const };
  assert.equal(applyDesktopOcrStatus(status, completed), completed);
  const next = { ...status, scan_id: 4, revision: 0, blocks: [] };
  assert.equal(applyDesktopOcrStatus(status, next), next);
  assert.equal(applyDesktopOcrStatus(null, status), status);
});

test("copy includes source and completed translations without pending placeholders", () => {
  assert.equal(desktopOcrCopyText(status.blocks), "Hello\ntranslated");
  assert.equal(desktopOcrCopyText([]), "");
  assert.equal(desktopOcrCopyText([...status.blocks, { source: { id: 2, text: "Goodbye", confidence: 1, polygon: [] }, translations: [] }]), "Hello\ntranslated\n\nGoodbye");
});
