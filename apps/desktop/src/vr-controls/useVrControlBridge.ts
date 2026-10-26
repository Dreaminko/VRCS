import { useEffect, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";
import type { Settings } from "../settings/types";
import { createControlExecutor } from "./actions";
import type { ControlRequest, ControlSnapshot, PlacementRequest } from "./protocol";
import type { PlayerAudioStart } from "../capture/usePlayerAudioStart";
import type { PlayerAudioStatus } from "../core-client/types";
import { playerAudioProgress, playerAudioPrompt, type PlayerAudioDecision } from "../capture/player-audio-vr";

export function useVrControlBridge(options: {
  settings: Settings | null;
  getSettings: () => Settings | null;
  update: (apply: (settings: Settings) => Settings) => Promise<Settings>;
  available: boolean;
  requested: boolean;
  getRequested: () => boolean;
  running: boolean;
  busy: boolean;
  setCaptureRunning: (target: boolean) => Promise<boolean>;
  playerAudioStart: PlayerAudioStart;
  playerAudioStatus?: PlayerAudioStatus | null;
}) {
  const { t } = useTranslation();
  const current = useRef(options);
  current.current = options;
  const [epoch, setEpoch] = useState(0);
  const revision = useRef(0);
  const audioRevision = useRef(0);
  const [now, setNow] = useState(() => performance.now());
  const view = useRef<ControlSnapshot>({ owner_epoch: 0, revision: 0, items: [], dark: false, labels: {} });
  const settings = options.getSettings();
  const overlay = settings?.vr_overlay;
  const item = (requested: boolean, effective: boolean, available: boolean, label: string, busy = false, reason: string | null = null) =>
    ({ requested, effective, available, busy, label, reason });
  view.current = {
    owner_epoch: epoch, revision: revision.current,
    dark: document.documentElement.dataset.theme === "dark",
    labels: Object.fromEntries(["emptyDialogue", "dragHint", "anchorWrist", "anchorWorld", "followView", "controlBusy", "controlFailed", "masterOff", "waitingContent"]
      .map((key) => [key, t(`settings.vrOverlay.${key}`)])),
    items: [
      item(overlay?.wrist.enabled ?? false, false, options.available && Boolean(settings?.features.vr_overlay), t("settings.vrDashboard.wrist"), false, overlay?.enabled ? null : t("settings.vrOverlay.masterOff")),
      item(overlay?.headset.enabled ?? false, false, options.available && Boolean(settings?.features.vr_overlay), t("settings.vrDashboard.headset"), false, overlay?.enabled ? null : t("settings.vrOverlay.masterOff")),
      item(settings?.osc.enabled ?? false, Boolean(settings?.features.osc_chatbox && settings.osc.enabled), options.available && Boolean(settings?.features.osc_chatbox), "OSC"),
      item(options.getRequested(), options.running, options.available, t(options.getRequested() ? "capture.stop" : "capture.start"), options.busy),
    ],
  };

  useEffect(() => {
    if (!isTauri()) return;
    let disposed = false;
    let owner = 0;
    const stops: Array<() => void> = [];
    const seen = new Map<string, number>();
    const waiting: Array<() => void> = [];
    const publish = async () => {
      const confirmed = current.current.getSettings();
      const nextView = { ...view.current, owner_epoch: owner, items: view.current.items.map((item) => ({ ...item })) };
      if (confirmed) {
        nextView.items[0].requested = confirmed.vr_overlay.wrist.enabled;
        nextView.items[1].requested = confirmed.vr_overlay.headset.enabled;
        nextView.items[2].requested = confirmed.osc.enabled;
      }
      nextView.items[3].requested = current.current.getRequested();
      const next = await invoke<ControlSnapshot>("vr_overlay_control_update", { view: nextView });
      revision.current = Math.max(revision.current, next.revision);
    };
    const execute = createControlExecutor({
      update: (apply) => current.current.update(apply),
      setCaptureRunning: (target) => current.current.setCaptureRunning(target),
    });
    void (async () => {
      const receive = (request: ControlRequest) => {
        if (!owner && !disposed) { waiting.push(() => receive(request)); return; }
        if (disposed || request.owner_epoch !== owner || request.request_id <= (seen.get(request.source) ?? 0)) return;
        seen.set(request.source, request.request_id);
        void (async () => {
          let error: string | null = null;
          try {
            if (request.base_revision < revision.current) throw new Error("State changed; try again");
            await execute(request.action);
          } catch (reason) {
            error = reason instanceof Error ? reason.message : String(reason);
          }
          if (disposed || owner !== request.owner_epoch) return;
          try {
            await publish();
          } finally {
            await invoke("vr_overlay_control_result", { request, error });
          }
        })().catch((error) => console.warn("VR control result failed", error));
      };
      stops.push(await listen<ControlRequest>("vr-overlay-control-request", ({ payload }) => receive(payload)));
      const place = (request: PlacementRequest) => {
        if (!owner && !disposed) { waiting.push(() => place(request)); return; }
        if (disposed || request.owner_epoch !== owner || request.id <= (seen.get("placement") ?? 0)) return;
        seen.set("placement", request.id);
        void (async () => {
          let error: string | null = null;
          try { await execute({ set_dialogue_placement: { mode: request.mode, follow_view: request.follow_view } }); }
          catch (reason) { error = reason instanceof Error ? reason.message : String(reason); }
          if (disposed || owner !== request.owner_epoch) return;
          await invoke("vr_overlay_placement_result", { epoch: owner, id: request.id, error });
        })().catch((error) => console.warn("VR placement result failed", error));
      };
      stops.push(await listen<PlacementRequest>("vr-overlay-placement-request", ({ payload }) => place(payload)));
      stops.push(await listen<PlayerAudioDecision>("vr-overlay-player-audio-decision", ({ payload }) => {
        if (!disposed && owner && payload.owner_epoch === owner) {
          current.current.playerAudioStart.decide(payload.accepted, payload.request_id);
        }
      }));
      if (disposed) { stops.forEach((stop) => stop()); return; }
      const ready = await invoke<ControlSnapshot>("vr_overlay_control_ready", { view: view.current });
      owner = ready.owner_epoch;
      if (disposed) { await invoke("vr_overlay_control_release", { epoch: owner }); return; }
      revision.current = ready.revision;
      setEpoch(owner);
      waiting.splice(0).forEach((receive) => receive());
    })().catch((error) => {
      stops.forEach((stop) => stop());
      if (owner) void invoke("vr_overlay_control_release", { epoch: owner });
      console.warn("VR control registration failed", error);
    });
    return () => {
      disposed = true;
      stops.forEach((stop) => stop());
      if (owner) void invoke("vr_overlay_control_release", { epoch: owner });
    };
  }, []);

  useEffect(() => {
    if (!options.playerAudioStart.notice) return;
    setNow(performance.now());
    const timer = window.setInterval(() => setNow(performance.now()), 200);
    return () => window.clearInterval(timer);
  }, [options.playerAudioStart.notice]);

  const notice = playerAudioPrompt(options.playerAudioStart.notice, now, t);
  const progress = notice ? null : playerAudioProgress(options.playerAudioStart.preparation, options.playerAudioStatus,
    settings?.audio.output.mode === "vrchat_players" && options.requested, t);
  const audioView = JSON.stringify({ notice, progress });
  useEffect(() => {
    if (!epoch) return;
    void invoke("vr_overlay_player_audio_update", { epoch, revision: ++audioRevision.current, view: JSON.parse(audioView) })
      .catch((error) => console.warn("VR player audio state update failed", error));
  }, [epoch, audioView]);

  useEffect(() => {
    if (!epoch) return;
    const publish = () => {
      view.current.dark = document.documentElement.dataset.theme === "dark";
      void invoke<ControlSnapshot>("vr_overlay_control_update", { view: view.current })
        .then((next) => { revision.current = Math.max(revision.current, next.revision); })
        .catch((error) => console.warn("VR control state update failed", error));
    };
    publish();
    const observer = new MutationObserver(publish);
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
    return () => observer.disconnect();
  }, [epoch, settings, options.available, options.requested, options.running, options.busy, t]);
}
