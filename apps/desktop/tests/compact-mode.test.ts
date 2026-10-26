import assert from "node:assert/strict";
import test from "node:test";
import {
  COMPACT_PANEL_WINDOW_SIZE,
  COMPACT_WINDOW_MAX_HEIGHT,
  COMPACT_WINDOW_MIN_WIDTH,
  COMPACT_WINDOW_SIZE,
  clampCompactWindowHeight,
  compactPartialMatchesSubtitle,
  compactPreviewText,
  compactLivePreview,
  compactWindowConstraints,
  compactWindowSize,
  subtitlesForCompactView,
} from "../src/shell/compact-mode.ts";
import type { Subtitle } from "../src/subtitles/types.ts";

const subtitles: Subtitle[] = [
  {
    id: 2,
    text: "latest subtitle",
    language: "en",
    source: "speaker",
    started_at: null,
    ended_at: null,
    created_at: "2026-07-21T10:01:00.000Z",
    translations: [],
  },
  {
    id: 1,
    text: "selected subtitle",
    language: "en",
    source: "speaker",
    started_at: null,
    ended_at: null,
    created_at: "2026-07-21T10:00:00.000Z",
    translations: [],
  },
  {
    id: 0,
    text: "older subtitle",
    language: "en",
    source: "speaker",
    started_at: null,
    ended_at: null,
    created_at: "2026-07-21T09:59:00.000Z",
    translations: [],
  },
  {
    id: null,
    text: "oldest subtitle",
    language: "en",
    source: "speaker",
    started_at: null,
    ended_at: null,
    created_at: "2026-07-21T09:58:00.000Z",
    translations: [],
  },
];

test("compact translation preview keeps a completed original even without source deltas", () => {
  const completed = { ...subtitles[0], utterance_id: "qwen-source-1" };
  const preview = {
    type: "partial" as const,
    source: "speaker" as const,
    utterance_id: "qwen-preview-1",
    source_utterance_id: "qwen-source-1",
    text: "",
    translation: "流式译文",
    target_language: "zh-Hans",
  };
  for (const text of ["", "  "]) {
    const resolved = compactLivePreview({ ...preview, text }, [completed]);
    assert.equal(resolved?.text, completed.text);
    assert.equal(resolved?.language, completed.language);
    assert.equal(resolved?.translation, preview.translation);
    assert.equal(resolved?.utterance_id, preview.utterance_id);
  }
  const sourceDelta = { ...preview, text: "original delta" };
  assert.deepEqual(compactLivePreview(sourceDelta, [completed]), { ...sourceDelta, language: completed.language });
  assert.equal(preview.text, "");
  assert.equal(compactLivePreview(preview, []), preview);
  assert.equal(compactLivePreview(preview, [{ ...completed, utterance_id: "other" }]), preview);
  assert.equal(compactLivePreview(preview, [{ ...completed, source: "microphone" }]), preview);
  assert.equal(compactLivePreview(null, [completed]), null);
});

test("compact mode supplies chronological candidates without mutating history", () => {
  const history = [...subtitles];
  const candidates = subtitlesForCompactView(history);
  assert.deepEqual(candidates, [...subtitles].reverse());
  assert.equal(candidates.at(-1), subtitles[0]);
  assert.deepEqual(history, subtitles);
  assert.notEqual(candidates, history);
});

test("compact previews recover originals after loaded history has been evicted", () => {
  const preview = {
    type: "partial" as const, source: "speaker" as const,
    utterance_id: "qwen-preview-older", source_utterance_id: "qwen-source-older",
    text: "", completed_original: "Older original.", translation: "迟到译文",
  };
  assert.equal(compactLivePreview(preview, [])?.text, "Older original.");
  const delta = { ...preview, text: "Original delta" };
  assert.equal(compactLivePreview(delta, []), delta);
});

test("late compact translation finds its original without changing candidate order", () => {
  const older = { ...subtitles[1], utterance_id: "qwen-source-older" };
  const history = [subtitles[0], older];
  const candidates = subtitlesForCompactView(history);
  assert.deepEqual(candidates, [older, subtitles[0]]);
  const preview = {
    type: "partial" as const,
    source: "speaker" as const,
    utterance_id: "qwen-preview-older",
    source_utterance_id: older.utterance_id,
    text: "",
    translation: "迟到的译文",
    target_language: "zh-Hans",
  };
  const resolved = compactLivePreview(preview, history);
  assert.equal(resolved?.text, older.text);
  assert.equal(resolved?.translation, preview.translation);
  assert.deepEqual(candidates, [older, subtitles[0]]);
});

test("compact mode freezes the selected subtitle while the selection panel is open", () => {
  assert.deepEqual(
    subtitlesForCompactView(subtitles, "selected subtitle"),
    [subtitles[1]],
  );
});

test("compact selection panel expands the current window without changing its default width", () => {
  assert.deepEqual(compactWindowSize(false), COMPACT_WINDOW_SIZE);
  assert.deepEqual(compactWindowSize(true), COMPACT_PANEL_WINDOW_SIZE);
  assert.equal(COMPACT_WINDOW_SIZE.width, COMPACT_PANEL_WINDOW_SIZE.width);
  assert.ok(COMPACT_PANEL_WINDOW_SIZE.height > COMPACT_WINDOW_SIZE.height);
});

test("compact selection panel preserves a user-resized width", () => {
  assert.deepEqual(compactWindowSize(false, 960, 240), {
    width: 960,
    height: 240,
  });
  assert.deepEqual(compactWindowSize(true, 960, 240), {
    width: 960,
    height: COMPACT_PANEL_WINDOW_SIZE.height,
  });
});

test("compact mode constrains width and normal subtitle height", () => {
  assert.deepEqual(compactWindowConstraints(false), {
    minWidth: COMPACT_WINDOW_MIN_WIDTH,
    minHeight: COMPACT_WINDOW_SIZE.height,
    maxHeight: COMPACT_WINDOW_MAX_HEIGHT,
  });
  assert.deepEqual(compactWindowConstraints(true), {
    minWidth: COMPACT_WINDOW_MIN_WIDTH,
    minHeight: COMPACT_PANEL_WINDOW_SIZE.height,
    maxHeight: COMPACT_PANEL_WINDOW_SIZE.height,
  });
  assert.equal("maxWidth" in compactWindowConstraints(false), false);
});

test("compact chatbox adds temporary space without replacing the subtitle height", () => {
  for (const [measured, total, minimum] of [[70, 310, 230], [210, 450, 370], [420, 660, 580]]) {
    assert.deepEqual(compactWindowSize(measured, 960, 240), { width: 960, height: total });
    assert.deepEqual(compactWindowConstraints(measured), {
      minWidth: 480, minHeight: minimum, maxHeight: 720,
    });
    assert.deepEqual(compactWindowSize(measured, 960, 680), { width: 960, height: 720 });
    assert.deepEqual(compactWindowSize(false, 960, 240), { width: 960, height: 240 });
  }
});

test("compact height is clamped to the supported range", () => {
  assert.equal(clampCompactWindowHeight(80), COMPACT_WINDOW_SIZE.height);
  assert.equal(clampCompactWindowHeight(241.6), 242);
  assert.equal(clampCompactWindowHeight(700), 700);
  assert.equal(clampCompactWindowHeight(900), COMPACT_WINDOW_MAX_HEIGHT);
});

test("compact selection falls back to the latest subtitle when history no longer contains its context", () => {
  assert.deepEqual(subtitlesForCompactView(subtitles, "evicted selection"), [subtitles[0]]);
  assert.deepEqual(subtitlesForCompactView([], "evicted selection"), []);
  assert.deepEqual(subtitlesForCompactView([]), []);
});

test("compact candidates retain growing text for the measurement layer", () => {
  const growing = subtitles.map((subtitle) => ({
    ...subtitle,
    text: subtitle.text.repeat(100),
    translation_partial: { text: "不断更新的译文。".repeat(100), target_language: "zh-Hans" },
  }));
  assert.deepEqual(
    subtitlesForCompactView(growing).map((subtitle) => subtitle.id),
    subtitlesForCompactView(subtitles).map((subtitle) => subtitle.id),
  );
  assert.equal(subtitlesForCompactView(growing).at(-1), growing[0]);
});

test("compact candidates are not capped at the former four-entry limit", () => {
  const history = [...subtitles, { ...subtitles[3], text: "fifth subtitle" }];
  assert.deepEqual(
    subtitlesForCompactView(history).map((subtitle) => subtitle.text),
    ["fifth subtitle", "oldest subtitle", "older subtitle", "selected subtitle", "latest subtitle"],
  );
});

test("long compact text remains complete for measurement and selection", () => {
  const text = "A long paragraph with detailed explanations. ".repeat(200) + "The latest sentence.";
  const item = { ...subtitles[0], text };
  const preview = compactPreviewText(item.text);
  assert.equal(preview, text);
  assert.equal(subtitlesForCompactView([item])[0].text, text);
});

test("compact windows keep unicode characters and ignore trailing blank lines", () => {
  const text = "🙂".repeat(601) + "\n\n ";
  const preview = compactPreviewText(text);
  assert.equal(preview, "🙂".repeat(601));
  assert.equal(compactPreviewText("短句。\n\n"), "短句。");
  assert.equal(compactPreviewText(" \n "), "");
  assert.equal(compactPreviewText("  first\nsecond\n"), "  first\nsecond");
});

test("compact partials match their source utterance and audio source", () => {
  const completed = { ...subtitles[0], utterance_id: "source-1" };
  const partial = {
    type: "partial" as const, source: "speaker" as const,
    utterance_id: "source-1", text: "preview",
  };
  assert.equal(compactPartialMatchesSubtitle(partial, completed), true);
  assert.equal(compactPartialMatchesSubtitle(partial, { ...completed, source: undefined }), true);
  assert.equal(compactPartialMatchesSubtitle(partial, { ...completed, source: "microphone" }), false);
  assert.equal(compactPartialMatchesSubtitle(partial, { ...completed, utterance_id: undefined }), false);
  assert.equal(compactPartialMatchesSubtitle({ ...partial, utterance_id: "preview-1", source_utterance_id: "source-1" }, completed), true);
  assert.equal(compactPartialMatchesSubtitle({ ...partial, source_utterance_id: "other-source" }, completed), false);
});

test("streaming and complete compact text both retain the latest translated tail", () => {
  const text = "非常长的技术说明。".repeat(100) + "这是最后一句。";
  assert.equal(compactPreviewText(text), compactPreviewText(text + "\n"));
  assert.ok(compactPreviewText(text + "补充。 ").endsWith("这是最后一句。补充。"));
});
