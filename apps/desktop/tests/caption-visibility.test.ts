import assert from "node:assert/strict";
import test from "node:test";
import { visibleCaptionMessages } from "../src/vr-surface/caption-visibility.ts";
import type { CaptionMessage, SurfaceView } from "../src/vr-surface/protocol.ts";

const message = (track: string, language: string | null, text = "Hello everyone", live = true, source = "speaker"): CaptionMessage => ({
  speaker: track, text: `${text}\nTranslated text`, side: source === "speaker" ? "left" : "right",
  original: { source, track_id: track, text, language, translation_sources: [], live },
});
const view = (messages: CaptionMessage[], kind: "headset" | "dialogue" = "headset") => ({
  kind, messages, max_visible_players: 2, max_entries: 3,
  visibility: { hidden_tracks: ["hidden"], hidden_source_languages: ["zh", "ja"] },
} as Pick<Extract<SurfaceView, { kind: "headset" | "dialogue" }>, "kind" | "messages" | "visibility" | "max_visible_players" | "max_entries">);

for (const kind of ["headset", "dialogue"] as const) {
  test(`${kind} applies shared source/track filtering before the player limit and preserves self`, () => {
    const messages = [message("hidden", "en"), message("chinese", "zh-CN"), message("japanese", "ja-JP"),
      message("english", "en"), message("french", "fr"), message("overflow", "en"), message("self", "zh", "自己的话", true, "microphone")];
    assert.deepEqual(visibleCaptionMessages(view(messages, kind)).map(item => item.speaker), ["english", "french", "self"]);
  });
  test(`${kind} filters missing source metadata from originals, including translation-only display`, () => {
    const chinese = message("chinese", null, "今天我们准备检查中文过滤功能。");
    chinese.text = "This is a synthetic translation for a filter test";
    const english = message("english", "en");
    english.text = "中文译文不决定原文语言";
    const unknown = message("unknown", null, "1234");
    const current = view([chinese, english, unknown], kind);
    assert.deepEqual(visibleCaptionMessages(current).map(item => item.speaker), ["english", "unknown"]);
    unknown.original!.translation_sources = ["Chinese"];
    assert.deepEqual(visibleCaptionMessages(current).map(item => item.speaker), ["english"]);
    current.visibility!.hidden_source_languages = [];
    assert.equal(visibleCaptionMessages(current).length, 2);
  });
  test(`${kind} filters final translations and allows runtime preference changes without new speech`, () => {
    const current = view([message("first", "en", "Earlier", false), message("hidden", "en", "Hidden", false), message("chinese", "zh", "你好", false)], kind);
    assert.deepEqual(visibleCaptionMessages(current).map(item => item.speaker), ["first"]);
    current.visibility = { hidden_tracks: [], hidden_source_languages: [] };
    current.max_visible_players = 0;
    assert.equal(visibleCaptionMessages(current).length, 3);
  });
}

test("headset keeps the latest visible language per player and replaces only its own lane", () => {
  const current = view([message("first", "en", "Earlier", false), message("first", "zh", "后来的中文", true), message("second", "en")]);
  assert.deepEqual(visibleCaptionMessages(current).map(item => item.original!.text), ["Hello everyone", "Earlier"]);
  current.max_visible_players = null;
  assert.deepEqual(visibleCaptionMessages(current).map(item => item.speaker), ["second"]);
});

test("wrist hidden history does not evict visible entries or parallel live previews", () => {
  const current = view([...Array.from({ length: 3 }, (_, i) => message(`past-${i}`, "en", "Past", false)),
    ...Array.from({ length: 6 }, (_, i) => message(`zh-${i}`, "zh", "隐藏", false))], "dialogue");
  current.max_visible_players = 0;
  assert.deepEqual(visibleCaptionMessages(current).map(item => item.speaker), ["past-0", "past-1", "past-2"]);
  current.messages.push(...Array.from({ length: 6 }, (_, i) => message(`active-${i}`, "en")));
  assert.equal(visibleCaptionMessages(current).length, 6);
});
