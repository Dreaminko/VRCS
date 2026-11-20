import { useEffect, useMemo, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";
import type { Settings } from "../settings/types";
import { createControlExecutor, photoGestureControlState } from "./actions";
import type { ControlRequest, ControlSnapshot, PlacementRequest } from "./protocol";
import type { PlayerAudioStart } from "../capture/usePlayerAudioStart";
import type { ChatboxStatus, PlayerAudioStatus } from "../core-client/types";
import { playerAudioProgress, playerAudioPrompt, type PlayerAudioDecision } from "../capture/player-audio-vr";
import { usesVrchatNetwork } from "../capture/vrchat-network";

const CONTROL_LABEL_KEYS = [
  "emptyDialogue", "dragHint", "anchorWrist", "anchorWorld", "followView",
  "controlBusy", "controlFailed", "masterOff", "waitingContent", "settings",
] as const;

interface VrControlBridgeOptions {
  settings: Settings | null;
  getSettings: () => Settings | null;
  update: (apply: (settings: Settings) => Settings) => Promise<Settings>;
  available: boolean;
  sceneCapture: boolean;
  requested: boolean | undefined;
  getRequested: () => boolean;
  running: boolean;
  busy: boolean;
  setCaptureRunning: (target: boolean) => Promise<boolean>;
  playerAudioStart: PlayerAudioStart;
  playerAudioStatus?: PlayerAudioStatus | null;
  chatboxStatus?: ChatboxStatus | null;
}

function buildControlView(
  options: VrControlBridgeOptions,
  ownerEpoch: number,
  revision: number,
  t: (key: string) => string,
): ControlSnapshot {
  const settings = options.getSettings();
  const overlay = settings?.vr_overlay;
  const item = (requested: boolean, effective: boolean, available: boolean, label: string, busy = false, reason: string | null = null, hidden = false) =>
    ({ requested, effective, available, busy, label, reason, hidden });
  const requested = options.getRequested();
  const gesture = photoGestureControlState(settings);
  return {
    owner_epoch: ownerEpoch,
    revision,
    dark: document.documentElement.dataset.theme === "dark",
    labels: Object.fromEntries(CONTROL_LABEL_KEYS.map((key) => [key, t(`settings.vrOverlay.${key}`)])),
    items: [
      item(overlay?.wrist.enabled ?? false, false, options.available && Boolean(settings?.features.vr_overlay), t("settings.vrDashboard.wrist"), false, overlay?.enabled ? null : t("settings.vrOverlay.masterOff")),
      item(overlay?.headset.enabled ?? false, false, options.available && Boolean(settings?.features.vr_overlay), t("settings.vrDashboard.headset"), false, overlay?.enabled ? null : t("settings.vrOverlay.masterOff")),
      item(settings?.osc.enabled ?? false, settings?.osc.enabled ?? false, options.available && Boolean(settings), "OSC"),
      item(requested, options.running, options.available, t(requested ? "capture.stop" : "capture.start"), options.busy),
      item(settings?.ocr.hand_gesture_enabled ?? false, settings?.ocr.hand_gesture_enabled ?? false,
        options.available && options.sceneCapture && gesture.available, t("settings.vrDashboard.gesture"), false, null, gesture.hidden),
    ],
  };
}

function controlKey(view: ControlSnapshot): string {
  return JSON.stringify({ dark: view.dark, labels: view.labels, items: view.items });
}

export function useVrControlBridge(options: VrControlBridgeOptions) {
  const { t } = useTranslation();
  const current = useRef(options);
  current.current = options;
  const tRef = useRef(t);
  tRef.current = t;
  const [epoch, setEpoch] = useState(0);
  const revision = useRef(0);
  const lastControlRef = useRef("");
  const audioRevision = useRef(0);
  const lastAudioViewRef = useRef("");
  const lastAudio = useRef<{ requested: boolean; status?: PlayerAudioStatus | ChatboxStatus | null; chatbox?: boolean }>({ requested: false });
  const [audioReadyHidden, setAudioReadyHidden] = useState(false);
  const [now, setNow] = useState(() => performance.now());

  useEffect(() => {
    if (!isTauri()) return;
    let disposed = false;
    let owner = 0;
    const stops: Array<() => void> = [];
    const seen = new Map<string, number>();
    const waiting: Array<() => void> = [];
    const publish = async () => {
      const nextView = buildControlView(current.current, owner, revision.current, tRef.current);
      lastControlRef.current = controlKey(nextView);
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
      const initialView = buildControlView(current.current, 0, revision.current, tRef.current);
      lastControlRef.current = controlKey(initialView);
      const ready = await invoke<ControlSnapshot>("vr_overlay_control_ready", { view: initialView });
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
      lastControlRef.current = "";
      stops.forEach((stop) => stop());
      if (owner) void invoke("vr_overlay_control_release", { epoch: owner });
    };
  }, []);

  useEffect(() => {
    if (!epoch || !options.playerAudioStart.notice) return;
    setNow(performance.now());
    const timer = window.setInterval(() => setNow(performance.now()), 200);
    return () => window.clearInterval(timer);
  }, [epoch, options.playerAudioStart.notice]);

  const settings = options.getSettings();
  const notice = epoch ? playerAudioPrompt(options.playerAudioStart.notice, now, t) : null;
  // A missing health snapshot is not a confirmed stop.
  const chatbox = settings?.audio.output.mode !== "vrchat_players" && Boolean(settings?.vrchat_chatbox?.enabled);
  const requested = usesVrchatNetwork(settings) && (options.requested ?? lastAudio.current.requested);
  const status = (chatbox ? options.chatboxStatus : options.playerAudioStatus)
    ?? (lastAudio.current.chatbox === chatbox ? lastAudio.current.status : null);
  lastAudio.current = { requested, status: requested ? status : null, chatbox };
  const ready = !notice && !options.playerAudioStart.preparation && requested && status?.phase === "streaming";
  useEffect(() => {
    setAudioReadyHidden(false);
    if (!epoch || !ready) return;
    const timer = window.setTimeout(() => setAudioReadyHidden(true), 2_000);
    return () => window.clearTimeout(timer);
  }, [epoch, ready]);
  const audioPayload = useMemo(() => {
    if (!epoch) return null;
    const progress = notice || (ready && audioReadyHidden) ? null
      : playerAudioProgress(options.playerAudioStart.preparation, status, requested, t, chatbox);
    const view = { notice, progress };
    return { serialized: JSON.stringify(view), view };
  }, [epoch, notice, ready, audioReadyHidden, options.playerAudioStart.preparation, status, requested, t, chatbox]);
  useEffect(() => {
    if (!epoch || !audioPayload) {
      lastAudioViewRef.current = "";
      return;
    }
    if (audioPayload.serialized === lastAudioViewRef.current) return;
    lastAudioViewRef.current = audioPayload.serialized;
    void invoke("vr_overlay_player_audio_update", { epoch, revision: ++audioRevision.current, view: audioPayload.view })
      .catch((error) => console.warn("VR player audio state update failed", error));
  }, [epoch, audioPayload]);

  const vrFeature = Boolean(settings?.features.vr_overlay);
  const overlayEnabled = Boolean(settings?.vr_overlay.enabled);
  const wristEnabled = Boolean(settings?.vr_overlay.wrist.enabled);
  const headsetEnabled = Boolean(settings?.vr_overlay.headset.enabled);
  const hasSettings = Boolean(settings);
  const oscEnabled = Boolean(settings?.osc.enabled);
  const ocrGestureEnabled = Boolean(settings?.ocr.hand_gesture_enabled);
  const { hidden: ocrGestureHidden, available: ocrGestureAvailable } = photoGestureControlState(settings);
  const captureRequested = options.getRequested();

  useEffect(() => {
    if (!epoch) return;
    const publish = () => {
      const nextView = buildControlView(current.current, epoch, revision.current, t);
      const serialized = controlKey(nextView);
      if (serialized === lastControlRef.current) return;
      lastControlRef.current = serialized;
      void invoke<ControlSnapshot>("vr_overlay_control_update", { view: nextView })
        .then((next) => { revision.current = Math.max(revision.current, next.revision); })
        .catch((error) => console.warn("VR control state update failed", error));
    };
    publish();
    const observer = new MutationObserver(publish);
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
    return () => observer.disconnect();
  }, [
    epoch,
    vrFeature,
    overlayEnabled,
    wristEnabled,
    headsetEnabled,
    hasSettings,
    oscEnabled,
    ocrGestureEnabled,
    ocrGestureHidden,
    ocrGestureAvailable,
    captureRequested,
    options.available,
    options.running,
    options.busy,
    options.sceneCapture,
    t,
  ]);

  return epoch;
}
