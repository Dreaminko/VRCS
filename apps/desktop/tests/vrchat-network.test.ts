import assert from "node:assert/strict";
import test from "node:test";
import type { TFunction } from "i18next";

import type { Health } from "../src/core-client/types.ts";
import type { Settings } from "../src/settings/types.ts";
import { usesVrchatNetwork } from "../src/capture/vrchat-network.ts";
import { vrchatNetworkStatus } from "../src/capture/vrchat-network-status.ts";

test("network preparation requires the master switch and a voice or Chatbox consumer", () => {
  for (const mode of ["system", "vrchat", "disabled", "vrchat_players"]) {
    for (const enabled of [false, true]) {
      const consumers = { audio: { output: { mode } }, vrchat_chatbox: { enabled } };
      assert.equal(usesVrchatNetwork(consumers), false, "A missing master switch defaults off");
      assert.equal(usesVrchatNetwork({ ...consumers, audio: { output: { mode, player_audio: { enabled: false } } } }), false);
      assert.equal(usesVrchatNetwork({ ...consumers, audio: { output: { mode, player_audio: { enabled: true } } } }), enabled || mode === "vrchat_players");
    }
  }
  assert.equal(usesVrchatNetwork(null), false);
  assert.equal(usesVrchatNetwork(undefined), false);
  assert.equal(usesVrchatNetwork({ audio: { output: { mode: "system" } } }), false);
});

test("the status bar reports the active VRChat network capture phase", () => {
  const t = ((key: string, values?: { players?: number }) =>
    values?.players === undefined ? key : `${key}:${values.players}`) as TFunction;
  const settings = {
    audio: { output: { mode: "vrchat_players", player_audio: { enabled: true } } },
    vrchat_chatbox: { enabled: false },
  } as Settings;
  const health = (
    phase: NonNullable<Health["player_audio"]>["phase"],
    activePlayers = 0,
    captureRequested = true,
  ) => ({
    capture_requested: captureRequested,
    player_audio: { phase, active_players: activePlayers },
  }) as Health;

  assert.equal(vrchatNetworkStatus(health("streaming"), {
    audio: { output: { mode: "system" } },
    vrchat_chatbox: { enabled: false },
  } as Settings, t), null);
  assert.equal(vrchatNetworkStatus(health("streaming"), {
    ...settings, audio: { output: { mode: "vrchat_players", player_audio: { enabled: false } } },
    vrchat_chatbox: { enabled: true },
  } as Settings, t), null);
  assert.deepEqual(vrchatNetworkStatus(null, settings, t), {
    label: "status.networkChecking", tone: "waiting",
  });
  assert.deepEqual(vrchatNetworkStatus(health("stopped", 0, false), settings, t), {
    label: "status.networkStopped", tone: "idle",
  });
  assert.equal(vrchatNetworkStatus(health("waiting_game"), settings, t)?.label, "status.networkWaitingGame");
  assert.equal(vrchatNetworkStatus(health("waiting_world"), settings, t)?.label, "status.networkWaitingWorld");
  assert.equal(vrchatNetworkStatus(health("waiting_traffic"), settings, t)?.label, "status.networkWaitingTraffic");
  assert.equal(vrchatNetworkStatus(health("acquiring_key"), settings, t)?.label, "status.networkAcquiringKey");
  assert.equal(vrchatNetworkStatus(health("retrying_key"), settings, t)?.label, "status.networkRetryingKey");
  assert.deepEqual(vrchatNetworkStatus(health("streaming", 2), settings, t), {
    label: "status.networkReceivingPlayers:2", tone: "active",
  });
  assert.deepEqual(vrchatNetworkStatus(health("streaming"), settings, t), {
    label: "status.networkWaitingPlayers", tone: "active",
  });
  const chatboxOnly = {
    capture_requested: true,
    player_audio: null,
    vrchat_chatbox: { phase: "streaming" },
  } as Health;
  assert.deepEqual(vrchatNetworkStatus(chatboxOnly, settings, t), {
    label: "status.networkReceivingChatbox", tone: "active",
  });
  assert.deepEqual(vrchatNetworkStatus(health("failed"), settings, t), {
    label: "status.networkFailed", tone: "error",
  });
});
