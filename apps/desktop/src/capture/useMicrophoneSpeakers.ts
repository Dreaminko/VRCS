import { useRef, useState, useSyncExternalStore } from "react";
import { useTranslation } from "react-i18next";
import { request } from "../core-client/transport";
import type { Settings } from "../settings/types";
import type { MicrophoneSpeakerAction, MicrophoneSpeakerSnapshot } from "./microphone-speakers";

let snapshot: MicrophoneSpeakerSnapshot | null = null;
const listeners = new Set<() => void>();
const subscribe = (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener); }; };

export function publishMicrophoneSpeakers(value: MicrophoneSpeakerSnapshot | null) {
  if (value && snapshot && value.snapshot_sequence < snapshot.snapshot_sequence) return;
  snapshot = value;
  listeners.forEach(listener => listener());
}

export function useMicrophoneSpeakers(connected: boolean, updateSettings: (apply: (settings: Settings) => Settings) => Promise<Settings>) {
  const { t } = useTranslation();
  const current = useSyncExternalStore(subscribe, () => snapshot, () => null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const inFlight = useRef(false);
  const latestConnected = useRef(connected);
  latestConnected.current = connected;
  const publishResponse = (value: MicrophoneSpeakerSnapshot) => {
    if (latestConnected.current && snapshot) publishMicrophoneSpeakers(value);
  };
  const execute = async (action: MicrophoneSpeakerAction) => {
    if (inFlight.current || !connected || !snapshot) throw new Error(t("live.microphoneSpeakers.unavailable"));
    inFlight.current = true;
    setBusy(true);
    setError(null);
    try {
      if ("set_microphone_filter" in action) {
        await updateSettings(settings => {
          if (!settings.features.microphone_speaker_filter) throw new Error(t("live.microphoneSpeakers.unavailable"));
          return { ...settings, audio: { ...settings.audio,
            microphone: { ...settings.audio.microphone, speaker_filter_enabled: action.set_microphone_filter } } };
        });
        publishResponse(await request<MicrophoneSpeakerSnapshot>("/api/capture/microphone/speakers"));
      } else {
        const { generation, revision, id, decision } = action.set_microphone_speaker;
        publishResponse(await request<MicrophoneSpeakerSnapshot>("/api/capture/microphone/speakers", {
          method: "PATCH", body: JSON.stringify({ generation, revision, selections: [{ id, decision }] }),
        }));
      }
    } catch (reason) {
      const message = t("live.microphoneSpeakers.updateFailed");
      setError(message);
      try { publishResponse(await request<MicrophoneSpeakerSnapshot>("/api/capture/microphone/speakers")); } catch { /* Reconnect supplies the next snapshot. */ }
      throw new Error(message, { cause: reason });
    } finally {
      inFlight.current = false;
      setBusy(false);
    }
  };
  return { snapshot: current, connected, busy, error, execute, getCurrent: () => snapshot };
}

export type MicrophoneSpeakerControl = ReturnType<typeof useMicrophoneSpeakers>;
