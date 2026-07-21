import type {
  AudioDevice,
  DictionaryEntry,
  Health,
  Settings,
  Subtitle,
} from "./types";

export const CORE_URL = "http://127.0.0.1:8765";
export const WS_URL = "ws://127.0.0.1:8765/ws";

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${CORE_URL}${path}`, {
    ...init,
    headers: { "Content-Type": "application/json", ...init?.headers },
  });
  if (!response.ok) {
    const body = (await response.json().catch(() => null)) as { detail?: string } | null;
    throw new Error(body?.detail ?? `${response.status} ${response.statusText}`);
  }
  return (await response.json()) as T;
}

export const coreApi = {
  health: () => request<Health>("/health"),
  subtitles: () => request<Subtitle[]>("/api/subtitles"),
  devices: () => request<AudioDevice[]>("/api/audio/devices"),
  settings: () => request<Settings>("/api/settings"),
  saveSettings: (settings: Settings) =>
    request<Settings>("/api/settings", {
      method: "PUT",
      body: JSON.stringify({
        asr: settings.asr,
        audio_device_id: settings.audio_device_id,
        microphone_device_id: settings.microphone_device_id,
      }),
    }),
  start: (deviceId: number | null, microphoneDeviceId: number | null) =>
    request<{ running: boolean; device: AudioDevice; microphone_device: AudioDevice | null }>("/api/capture/start", {
      method: "POST",
      body: JSON.stringify({
        device_id: deviceId,
        microphone_device_id: microphoneDeviceId,
      }),
    }),
  stop: () => request<{ running: boolean }>("/api/capture/stop", { method: "POST" }),
  lookup: (term: string) =>
    request<DictionaryEntry[]>(`/api/dictionary?q=${encodeURIComponent(term)}`),
  createCard: (front: string, back: string, context: string) =>
    request<{ note_id: number }>("/api/anki/cards", {
      method: "POST",
      body: JSON.stringify({ front, back, context }),
    }),
};
