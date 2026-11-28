import { useEffect, useMemo, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";
import type { Settings } from "../settings/types";
import { createControlExecutor, photoGestureControlState } from "./actions";
import type { ControlRequest, ControlSnapshot, LayoutEditCommitRequest, PlacementRequest } from "./protocol";
import type { PlayerAudioStart } from "../capture/usePlayerAudioStart";
import type { ChatboxStatus, PlayerAudioStatus } from "../core-client/types";
import { playerAudioProgress, playerAudioPrompt, type PlayerAudioDecision } from "../capture/player-audio-vr";
import { networkCaptureFeatureEnabled, usesVrchatNetwork } from "../capture/vrchat-network";

const CONTROL_LABEL_KEYS = [
  "emptyDialogue", "dragHint", "anchorWrist", "anchorWorld", "followView",
  "controlBusy", "controlFailed", "masterOff", "waitingContent", "settings",
] as const;

type DashboardControlResultListener = (error: string | null) => void;
const dashboardControlResultListeners = new Set<DashboardControlResultListener>();

export function subscribeDashboardControlResults(
  listener: DashboardControlResultListener,
): () => void {
  dashboardControlResultListeners.add(listener);
  return () => {
    dashboardControlResultListeners.delete(listener);
  };
}

interface VrControlBridgeOptions {
  os?: string;
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
  networkVisible?: boolean;
  networkRequested?: boolean | undefined;
  getNetworkRequested?: () => boolean;
  networkRunning?: boolean;
  networkBusy?: boolean;
  setNetworkCaptureRunning?: (target: boolean) => Promise<boolean>;
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
  const networkVisible = options.networkVisible ?? networkCaptureFeatureEnabled(settings);
  const networkRequested = options.getNetworkRequested?.() ?? Boolean(options.networkRequested);
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
      item(networkRequested, options.networkRunning ?? false, options.available && networkVisible, t(networkRequested ? "capture.networkStop" : "capture.networkStart"), options.networkBusy ?? false, null, !networkVisible),
    ],
  };
}

function controlKey(view: ControlSnapshot): string {
  return JSON.stringify({ dark: view.dark, labels: view.labels, items: view.items });
}

function controlStateKey(view: ControlSnapshot): string {
  return JSON.stringify(view.items.map(({ requested, available, hidden }) => [requested, available, Boolean(hidden)]));
}

export function useVrControlBridge(options: VrControlBridgeOptions) {
  const { t } = useTranslation();
  const current = useRef(options);
  current.current = options;
  const tRef = useRef(t);
  tRef.current = t;
  const [epoch, setEpoch] = useState(0);
  const revision = useRef(0);
  const stateRevision = useRef(0);
  const lastControlRef = useRef("");
  const lastControlStateRef = useRef("");
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
      lastControlStateRef.current = controlStateKey(nextView);
      const next = await invoke<ControlSnapshot>("vr_overlay_control_update", { view: nextView });
      revision.current = Math.max(revision.current, next.revision);
      stateRevision.current = Math.max(stateRevision.current, next.revision);
    };
    const execute = createControlExecutor({
      update: (apply) => current.current.update(apply),
      setCaptureRunning: (target) => current.current.setCaptureRunning(target),
      setNetworkCaptureRunning: (target) => current.current.setNetworkCaptureRunning?.(target) ?? Promise.resolve(false),
    });
    void (async () => {
      const receive = (request: ControlRequest) => {
        if (!owner && !disposed) { waiting.push(() => receive(request)); return; }
        if (disposed || request.owner_epoch !== owner || request.request_id <= (seen.get(request.source) ?? 0)) return;
        seen.set(request.source, request.request_id);
        void (async () => {
          let error: string | null = null;
          try {
            if (request.base_revision < stateRevision.current) throw new Error("State changed; try again");
            await execute(request.action);
          } catch (reason) {
            error = reason instanceof Error ? reason.message : String(reason);
          }
          if (disposed || owner !== request.owner_epoch) return;
          try {
            await publish();
          } catch (publishError) {
            console.warn("VR control publish failed", publishError);
          }
          if (disposed || owner !== request.owner_epoch) return;
          await invoke("vr_overlay_control_result", { request, error });
          if (request.source === "dashboard") {
            dashboardControlResultListeners.forEach((listener) => listener(error));
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
      const commitLayoutEdit = (request: LayoutEditCommitRequest) => {
        if (!owner && !disposed) { waiting.push(() => commitLayoutEdit(request)); return; }
        if (disposed || request.owner_epoch !== owner || request.id <= (seen.get("layout_edit") ?? 0)) return;
        seen.set("layout_edit", request.id);
        void (async () => {
          let error: string | null = null;
          try {
            await current.current.update((settings) => ({
              ...settings,
              vr_overlay: {
                ...settings.vr_overlay,
                headset: request.headset ? { ...settings.vr_overlay.headset, ...request.headset } : settings.vr_overlay.headset,
                wrist: request.wrist ? { ...settings.vr_overlay.wrist, ...request.wrist } : settings.vr_overlay.wrist,
                wrist_menu: request.wrist_menu
                  ? { ...settings.vr_overlay.wrist_menu, ...request.wrist_menu }
                  : settings.vr_overlay.wrist_menu,
              },
            }));
          } catch (reason) {
            error = reason instanceof Error ? reason.message : String(reason);
          }
          if (disposed || owner !== request.owner_epoch) return;
          await invoke("vr_overlay_layout_edit_result", { epoch: owner, id: request.id, error });
        })().catch((error) => console.warn("VR layout edit result failed", error));
      };
      stops.push(await listen<LayoutEditCommitRequest>("vr-overlay-layout-edit-commit", ({ payload }) => commitLayoutEdit(payload)));
      stops.push(await listen<PlayerAudioDecision>("vr-overlay-player-audio-decision", ({ payload }) => {
        if (!disposed && owner && payload.owner_epoch === owner) {
          current.current.playerAudioStart.decide(payload.accepted, payload.request_id);
        }
      }));
      if (disposed) { stops.forEach((stop) => stop()); return; }
      const initialView = buildControlView(current.current, 0, revision.current, tRef.current);
      lastControlRef.current = controlKey(initialView);
      lastControlStateRef.current = controlStateKey(initialView);
      const ready = await invoke<ControlSnapshot>("vr_overlay_control_ready", { view: initialView });
      owner = ready.owner_epoch;
      if (disposed) { await invoke("vr_overlay_control_release", { epoch: owner }); return; }
      revision.current = ready.revision;
      stateRevision.current = ready.revision;
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
      lastControlStateRef.current = "";
      stops.forEach((stop) => stop());
      if (owner) void invoke("vr_overlay_control_release", { epoch: owner });
    };
  }, []);

  useEffect(() => {
    const notice = options.playerAudioStart.notice;
    if (!epoch || !notice) return;
    const currentNow = performance.now();
    setNow(currentNow);
    if (currentNow - notice.openedAt >= 3_000) return;
    const timer = window.setInterval(() => {
      const nextNow = performance.now();
      setNow(nextNow);
      if (nextNow - notice.openedAt >= 3_000) window.clearInterval(timer);
    }, 200);
    return () => window.clearInterval(timer);
  }, [epoch, options.playerAudioStart.notice]);

  const settings = options.getSettings();
  const notice = epoch ? playerAudioPrompt(options.playerAudioStart.notice, now, t, options.os) : null;
  // A missing health snapshot is not a confirmed stop.
  const chatbox = (options.requested ?? options.getRequested())
    && settings?.audio.output.mode !== "vrchat_players"
    && Boolean(settings?.vrchat_chatbox.enabled);
  const networkRequested = options.getNetworkRequested?.() ?? Boolean(options.networkRequested);
  const requested = networkRequested || (usesVrchatNetwork(settings) && (options.requested ?? lastAudio.current.requested));
  const status = (chatbox ? options.chatboxStatus : options.playerAudioStatus)
    ?? options.playerAudioStatus
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
  const networkVisible = options.networkVisible ?? networkCaptureFeatureEnabled(settings);

  useEffect(() => {
    if (!epoch) return;
    const publish = () => {
      const nextView = buildControlView(current.current, epoch, revision.current, t);
      const serialized = controlKey(nextView);
      if (serialized === lastControlRef.current) return;
      lastControlRef.current = serialized;
      const nextStateKey = controlStateKey(nextView);
      const stateChanged = nextStateKey !== lastControlStateRef.current;
      lastControlStateRef.current = nextStateKey;
      void invoke<ControlSnapshot>("vr_overlay_control_update", { view: nextView })
        .then((next) => {
          revision.current = Math.max(revision.current, next.revision);
          if (stateChanged) stateRevision.current = Math.max(stateRevision.current, next.revision);
        })
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
    networkVisible,
    networkRequested,
    options.available,
    options.running,
    options.busy,
    options.networkRunning,
    options.networkBusy,
    options.sceneCapture,
    t,
  ]);

  return epoch;
}
