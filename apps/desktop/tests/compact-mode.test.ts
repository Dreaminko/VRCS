import assert from "node:assert/strict";
import test from "node:test";
import {
  COMPACT_PANEL_WINDOW_SIZE,
  COMPACT_PREVIEW_MAX_CHARS,
  COMPACT_SUBTITLE_MAX_ITEMS,
  COMPACT_SUBTITLE_HEIGHT_STEP,
  COMPACT_WINDOW_MAX_HEIGHT,
  COMPACT_WINDOW_MIN_WIDTH,
  COMPACT_WINDOW_SIZE,
  clampCompactWindowHeight,
  compactSubtitleCount,
  compactPreviewText,
  compactLivePreview,
  compactWindowConstraints,
  compactWindowSize,
  subtitlesForCompactView,
} from "../src/compact-mode.ts";
import type { Subtitle } from "../src/types.ts";

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
  assert.equal(compactLivePreview(sourceDelta, [completed]), sourceDelta);
  assert.equal(preview.text, "");
  assert.equal(compactLivePreview(preview, []), preview);
  assert.equal(compactLivePreview(preview, [{ ...completed, utterance_id: "other" }]), preview);
  assert.equal(compactLivePreview(preview, [{ ...completed, source: "microphone" }]), preview);
  assert.equal(compactLivePreview(null, [completed]), null);
});

test("compact mode follows the latest subtitle when the selection panel is closed", () => {
  assert.deepEqual(subtitlesForCompactView(subtitles, 120), [subtitles[0]]);
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

test("late compact translation finds its original outside the visible history", () => {
  const older = { ...subtitles[1], utterance_id: "qwen-source-older" };
  const history = [subtitles[0], older];
  const visible = subtitlesForCompactView(history, COMPACT_WINDOW_SIZE.height);
  assert.deepEqual(visible, [subtitles[0]]);
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
  assert.deepEqual(visible, [subtitles[0]]);
});

test("compact mode freezes the selected subtitle while the selection panel is open", () => {
  assert.deepEqual(
    subtitlesForCompactView(subtitles, 360, "selected subtitle"),
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

test("compact height is clamped to the supported range", () => {
  assert.equal(clampCompactWindowHeight(80), COMPACT_WINDOW_SIZE.height);
  assert.equal(clampCompactWindowHeight(241.6), 242);
  assert.equal(clampCompactWindowHeight(700), 700);
  assert.equal(clampCompactWindowHeight(900), COMPACT_WINDOW_MAX_HEIGHT);
});

test("compact subtitle count adds history only when two complete lines fit in both languages", () => {
  for (const [height, count] of [
    [160, 1], [269, 1], [270, 2], [377, 2],
    [378, 3], [460, 3], [485, 3], [486, 4], [720, 4],
  ]) {
    assert.equal(compactSubtitleCount(height), count, 'height ' + height);
  }
  assert.equal(compactSubtitleCount(COMPACT_WINDOW_MAX_HEIGHT), COMPACT_SUBTITLE_MAX_ITEMS);
  assert.ok(COMPACT_SUBTITLE_HEIGHT_STEP >= 2 * 24 * 2 + 4 + 6 + 1,
    "each entry needs room for two lines in each language plus its spacing");
});

test("compact subtitle capacity stays stable as originals and translations grow", () => {
  const growing = subtitles.map((subtitle) => ({
    ...subtitle,
    text: subtitle.text.repeat(100),
    translation_partial: { text: "不断更新的译文。".repeat(100), target_language: "zh-Hans" },
  }));
  assert.deepEqual(
    subtitlesForCompactView(growing, 460).map((subtitle) => subtitle.id),
    subtitlesForCompactView(subtitles, 460).map((subtitle) => subtitle.id),
  );
  assert.equal(subtitlesForCompactView(growing, 460).length, 3);
});

test("compact subtitle context is chronological and bounded by height", () => {
  assert.deepEqual(
    subtitlesForCompactView(subtitles, 460).map((subtitle) => subtitle.text),
    ["older subtitle", "selected subtitle", "latest subtitle"],
  );
  assert.deepEqual(
    subtitlesForCompactView([...subtitles, { ...subtitles[3], text: "beyond limit" }], 720)
      .map((subtitle) => subtitle.text),
    ["oldest subtitle", "older subtitle", "selected subtitle", "latest subtitle"],
  );
});

test("long compact text retains its newest end without changing full history", () => {
  const text = "A long paragraph with detailed explanations. ".repeat(200) + "The latest sentence.";
  const item = { ...subtitles[0], text };
  const preview = compactPreviewText(item.text);
  assert.equal(Array.from(preview).length, COMPACT_PREVIEW_MAX_CHARS);
  assert.ok(preview.endsWith("The latest sentence."));
  assert.equal(subtitlesForCompactView([item], 120)[0].text, text);
});

test("compact windows keep unicode characters and ignore trailing blank lines", () => {
  const text = "🙂".repeat(COMPACT_PREVIEW_MAX_CHARS + 1) + "\n\n ";
  const preview = compactPreviewText(text);
  assert.equal(preview, "🙂".repeat(COMPACT_PREVIEW_MAX_CHARS));
  assert.equal(compactPreviewText("短句。\n\n"), "短句。");
  assert.equal(compactPreviewText(" \n "), "");
});

test("streaming and complete compact text both retain the latest translated tail", () => {
  const text = "非常长的技术说明。".repeat(100) + "这是最后一句。";
  assert.equal(compactPreviewText(text), compactPreviewText(text + "\n"));
  assert.ok(compactPreviewText(text + "补充。 ").endsWith("这是最后一句。补充。"));
});
