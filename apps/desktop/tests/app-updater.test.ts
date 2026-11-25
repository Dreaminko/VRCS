import assert from "node:assert/strict";
import test from "node:test";

import { updaterErrorCode } from "../src/updates/app-updater.ts";
import {
  extractNotesPreview,
  formatBytes,
  formatDownloadSpeed,
  formatPublishDate,
  isNightlyVersion,
  resolveUpdateErrorKey,
  stripPlatformNotesSection,
} from "../src/updates/update-notes.ts";

test("normalizes native updater errors to localization keys", () => {
  assert.equal(updaterErrorCode("update.no_pending"), "no_pending");
  assert.equal(updaterErrorCode("update.timeout"), "timeout");
  assert.equal(updaterErrorCode("update.download_failed"), "download_failed");
  assert.equal(updaterErrorCode("update.signature_invalid"), "signature_invalid");
  assert.equal(updaterErrorCode("update.install_failed"), "install_failed");
  assert.equal(updaterErrorCode("update.unknown_code"), "failed");
  assert.equal(updaterErrorCode(new Error("network failed")), "failed");
  assert.equal(resolveUpdateErrorKey("signature_invalid"), "signature_invalid");
  assert.equal(resolveUpdateErrorKey("bogus"), "failed");
});

test("strips automated platform appendix and nightly footer from release notes", () => {
  const source = [
    "### Features",
    "- Added rich update details dialog.",
    "",
    "<!-- vrcs-platform-notes-start -->",
    "<details>",
    "<summary>Platform &amp; Installation Notes</summary>",
    "Boilerplate",
    "</details>",
    "<!-- vrcs-platform-notes-end -->",
  ].join("\n");
  assert.equal(
    stripPlatformNotesSection(source),
    "### Features\n- Added rich update details dialog.",
  );

  const nightlyBody = [
    "VRCS 0.4.1-nightly.20261125 Nightly",
    "",
    "### Fixes",
    "- Fixed subtitle scroll.",
    "",
    "---",
    "",
    "Build date: 2026-11-25 (Beijing time)",
    "Source commit: `abc1234`",
  ].join("\n");
  assert.equal(
    stripPlatformNotesSection(nightlyBody),
    "VRCS 0.4.1-nightly.20261125 Nightly\n\n### Fixes\n- Fixed subtitle scroll.",
  );
});

test("extracts clean preview bullet points from markdown release notes", () => {
  const notes = [
    "### Features",
    "- **Updater**: Show [release notes](https://github.com/Dreaminko/VRCS/releases) inside the toast.",
    "- Support `skipVersion` persistence.",
    "",
    "### Fixes",
    "- Keep pending update on retry.",
    "- Fourth item should be omitted.",
  ].join("\n");

  assert.deepEqual(extractNotesPreview(notes, 3), [
    "Updater: Show release notes inside the toast.",
    "Support skipVersion persistence.",
    "Keep pending update on retry.",
  ]);
  assert.deepEqual(extractNotesPreview("", 3), []);
});

test("formats bytes, speeds, publish dates, and nightly versions", () => {
  assert.equal(formatBytes(0), "0 B");
  assert.equal(formatBytes(512), "512 B");
  assert.equal(formatBytes(1536), "2 KB");
  assert.equal(formatBytes(5 * 1024 * 1024), "5.0 MB");
  assert.equal(formatDownloadSpeed(2.5 * 1024 * 1024), "2.5 MB/s");
  assert.equal(formatDownloadSpeed(0), null);
  assert.ok(formatPublishDate("2026-11-25T08:00:00Z", "en-US"));
  assert.equal(formatPublishDate("not-a-date", "en-US"), null);
  assert.equal(isNightlyVersion("0.4.1-nightly.20261125"), true);
  assert.equal(isNightlyVersion("0.4.1"), false);
});
