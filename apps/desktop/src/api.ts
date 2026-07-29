import type {
  AnkiCardInput,
  AnkiStatus,
  AsrCapabilities,
  AsrModelRecord,
  AudioDevice,
  DictionaryEntry,
  DictionarySource,
  Health,
  Settings,
  Subtitle,
} from "./types";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { apiErrorFromResponse } from "./api-error";

interface CoreConnection {
  httpUrl: string;
  wsUrl: string;
  token: string;
}

let connection: CoreConnection = {
  httpUrl: "http://127.0.0.1:8766",
  wsUrl: "ws://127.0.0.1:8766/ws",
  token: "",
};

export async function initializeCoreApi(): Promise<void> {
  if (isTauri()) {
    connection = await invoke<CoreConnection>("core_connection");
  }
}

function requestHeaders(initial?: HeadersInit): Headers {
  const headers = new Headers(initial);
  if (!headers.has("Content-Type")) headers.set("Content-Type", "application/json");
  if (connection.token) headers.set("Authorization", `Bearer ${connection.token}`);
  return headers;
}

export function coreWebSocketUrl(): string {
  const url = new URL(connection.wsUrl);
  if (connection.token) url.searchParams.set("token", connection.token);
  return url.toString();
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${connection.httpUrl}${path}`, {
    ...init,
    headers: requestHeaders(init?.headers),
  });
  if (!response.ok) {
    throw await apiErrorFromResponse(response);
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
      body: JSON.stringify(settings),
    }),
  start: () =>
    request<{ running: boolean; device: AudioDevice; microphone_device: AudioDevice | null }>("/api/capture/start", {
      method: "POST",
      body: JSON.stringify({}),
    }),
  stop: () => request<{ running: boolean }>("/api/capture/stop", { method: "POST" }),
  asrCapabilities: () => request<AsrCapabilities>("/api/asr/capabilities"),
  asrModels: () => request<AsrModelRecord[]>("/api/asr/models"),
  downloadAsrModel: (model: AsrModelRecord["id"]) =>
    request<AsrModelRecord>(`/api/asr/models/${model}/download`, {
      method: "POST",
      body: JSON.stringify({}),
    }),
  deleteAsrModel: (model: AsrModelRecord["id"]) =>
    request<{ deleted: boolean }>(`/api/asr/models/${model}`, { method: "DELETE" }),
  lookup: (term: string) =>
    request<DictionaryEntry[]>(`/api/dictionary?q=${encodeURIComponent(term)}`),
  dictionaries: () => request<DictionarySource[]>("/api/dictionaries"),
  importDictionary: async (file: File) => {
    const response = await fetch(`${connection.httpUrl}/api/dictionaries/import`, {
      method: "POST",
      headers: requestHeaders({ "Content-Type": "application/zip" }),
      body: file,
    });
    if (!response.ok) {
      throw await apiErrorFromResponse(response);
    }
    return (await response.json()) as DictionarySource;
  },
  deleteDictionary: (id: number) =>
    request<{ deleted: boolean }>(`/api/dictionaries/${id}`, { method: "DELETE" }),
  ankiStatus: () => request<AnkiStatus>("/api/anki/status"),
  createCard: (card: AnkiCardInput) =>
    request<{ note_id: number }>("/api/anki/cards", {
      method: "POST",
      body: JSON.stringify(card),
    }),
};
