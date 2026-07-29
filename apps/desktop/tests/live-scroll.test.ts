import assert from "node:assert/strict";
import test from "node:test";
import { shouldFollowLiveScroll } from "../src/live-scroll.ts";

test("stops following as soon as the user scrolls upward", () => {
  assert.equal(shouldFollowLiveScroll(true, {
    scrollTop: 499,
    previousScrollTop: 520,
    scrollHeight: 1000,
    clientHeight: 480,
  }), false);
});

test("keeps following while new content increases the distance from the bottom", () => {
  assert.equal(shouldFollowLiveScroll(true, {
    scrollTop: 520,
    previousScrollTop: 520,
    scrollHeight: 1100,
    clientHeight: 480,
  }), true);
});

test("resumes following only after the user returns to the bottom", () => {
  assert.equal(shouldFollowLiveScroll(false, {
    scrollTop: 400,
    previousScrollTop: 360,
    scrollHeight: 1000,
    clientHeight: 480,
  }), false);
  assert.equal(shouldFollowLiveScroll(false, {
    scrollTop: 500,
    previousScrollTop: 400,
    scrollHeight: 1000,
    clientHeight: 480,
  }), true);
});
