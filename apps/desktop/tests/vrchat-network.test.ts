import assert from "node:assert/strict";
import test from "node:test";
import { usesVrchatNetwork } from "../src/capture/vrchat-network.ts";

test("network preparation applies to player voice or Chatbox independently", () => {
  for (const mode of ["system", "vrchat", "disabled", "vrchat_players"]) {
    for (const enabled of [false, true]) {
      assert.equal(usesVrchatNetwork({ audio: { output: { mode } }, vrchat_chatbox: { enabled } }), enabled || mode === "vrchat_players");
    }
  }
  assert.equal(usesVrchatNetwork(null), false);
  assert.equal(usesVrchatNetwork(undefined), false);
  assert.equal(usesVrchatNetwork({ audio: { output: { mode: "system" } } }), false);
});
