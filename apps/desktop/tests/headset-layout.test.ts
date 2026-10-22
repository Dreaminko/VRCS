import assert from "node:assert/strict";
import test from "node:test";
import { visibleCaption, visibleTail } from "../src/vr-surface/headset-layout.ts";

const segmenter = new Intl.Segmenter(undefined, { granularity: "grapheme" });
const fits = (limit: number) => (text: string) => [...segmenter.segment(text)].length <= limit;

test("growing captions retain the same newest text instead of shrinking it", () => {
  const suffix = "最新字幕仍然清晰可读";
  assert.equal(visibleTail(`旧内容${suffix}`, fits(10)), "…新字幕仍然清晰可读");
  assert.equal(visibleTail(`${"旧内容".repeat(100)}${suffix}`, fits(10)), "…新字幕仍然清晰可读");
  assert.equal(visibleTail("短句", fits(10)), "短句");
  assert.equal(visibleTail("", fits(0)), "");
});

test("caption truncation preserves speaker labels and complete Unicode graphemes", () => {
  const text = "之前的内容 👨‍👩‍👧‍👦e\u0301 🇯🇵";
  assert.equal(visibleTail(text, fits(6)), "…👨‍👩‍👧‍👦e\u0301 🇯🇵");
  assert.equal(visibleCaption(`[12] ${text}`, fits(11)), "[12] …👨‍👩‍👧‍👦e\u0301 🇯🇵");
  assert.equal(visibleCaption("[2] long caption", fits(3)), "[2]");
  assert.equal(visibleCaption("[2] long caption", fits(2)), "");
});

test("ordinary brackets are not treated as numeric speaker labels", () => {
  for (const text of ["普通字幕保持结尾", "[note] long caption", "[] long caption", "[2]caption"]) {
    assert.equal(visibleCaption(text, fits(8)), visibleTail(text, fits(8)));
  }
});

test("English captions prefer whole words but keep the end of oversized tokens", () => {
  assert.equal(visibleTail("old prefix vocabulary latest words", fits(17)), "…latest words");
  const token = visibleTail("https://example.com/very-long-token", fits(17));
  assert.ok(token.endsWith("very-long-token"));
  assert.ok(fits(17)(token));
});

test("word boundary cleanup must still fit the measured multiline window", () => {
  const fitsWindow = (text: string) => {
    let lines = 1;
    let width = 0;
    for (const word of text.split(/\s+/)) {
      const size = [...segmenter.segment(word)].length;
      if (size > 8) return false;
      if (width && width + 1 + size > 8) { lines++; width = size; }
      else width += (width ? 1 : 0) + size;
    }
    return lines <= 2;
  };
  assert.equal(visibleTail("old 1234567890 abcdefgh", fitsWindow), "…4567890 abcdefgh");
});
