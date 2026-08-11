import type {
  AnkiCardInput,
  AnkiStatus,
  ApiProfile,
  ApiProfileView,
  ApiModelCatalog,
  AsrApiProvider,
  AsrCapabilities,
  AsrModelRecord,
  AudioDevice,
  DictionaryEntry,
  DictionaryImportProgress,
  DictionarySource,
  Health,
  Settings,
  Subtitle,
  SubtitleTranslation,
} from "./types";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { apiErrorFromResponse } from "./api-error";

interface CoreConnection {
  httpUrl: string;
  wsUrl: string;
  token: string;
}

export interface CoreStartup {
  state: "starting" | "ready" | "failed";
  error: string | null;
}

let connection: CoreConnection = {
  httpUrl: "http://127.0.0.1:8766",
  wsUrl: "ws://127.0.0.1:8766/ws",
  token: import.meta.env.VITE_VRCS_SESSION_TOKEN ?? "",
};
interface ConfigRevision {
  token: string;
  epoch: string;
  counter: number;
}

let configRevision: ConfigRevision | null = null;

export async function initializeCoreApi(): Promise<void> {
  configRevision = null;
  if (isTauri()) {
    connection = await invoke<CoreConnection>("core_connection");
  }
}

export async function coreStartup(): Promise<CoreStartup> {
  if (!isTauri()) return { state: "ready", error: null };
  return invoke<CoreStartup>("core_startup");
}

export async function retryCore(): Promise<void> {
  if (!isTauri()) return;
  await invoke("retry_core");
  configRevision = null;
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

function parseConfigRevision(value: string | null): ConfigRevision | null {
  if (value === null) return null;
  const separator = value.lastIndexOf(":");
  if (separator <= 0) return null;
  const epoch = value.slice(0, separator);
  const counter = Number.parseInt(value.slice(separator + 1), 10);
  if (!Number.isSafeInteger(counter) || counter < 0) return null;
  return { token: value, epoch, counter };
}

async function settingsRequest(
  init?: RequestInit,
  retryStaleResponse = true,
): Promise<Settings> {
  const headers = requestHeaders(init?.headers);
  if (init?.method === "PUT" && configRevision !== null) {
    headers.set("X-VRCS-Config-Revision", configRevision.token);
  }
  const response = await fetch(`${connection.httpUrl}/api/settings`, {
    ...init,
    headers,
  });
  if (!response.ok) {
    throw await apiErrorFromResponse(response);
  }
  const responseRevision = parseConfigRevision(
    response.headers.get("X-VRCS-Config-Revision"),
  );
  if (
    responseRevision !== null
    && configRevision !== null
    && responseRevision.epoch === configRevision.epoch
    && responseRevision.counter < configRevision.counter
  ) {
    if (retryStaleResponse) return settingsRequest(undefined, false);
    throw new Error("The Core returned an outdated settings revision");
  }
  if (responseRevision !== null && (
    configRevision === null
    || responseRevision.epoch !== configRevision.epoch
    || responseRevision.counter >= configRevision.counter
  )) {
    configRevision = responseRevision;
  }
  return (await response.json()) as Settings;
}

function dictionaryImportId(): string {
  if (typeof crypto.randomUUID === "function") return crypto.randomUUID();
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
}

function delay(milliseconds: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, milliseconds));
}

export const coreApi = {
  health: () => request<Health>("/health"),
  subtitles: () => request<Subtitle[]>("/api/subtitles"),
  devices: () => request<AudioDevice[]>("/api/audio/devices"),
  settings: () => settingsRequest(),
  saveSettings: (settings: Settings) =>
    settingsRequest({
      method: "PUT",
      body: JSON.stringify(settings),
    }),
  start: () =>
    request<{ running: boolean; device: AudioDevice; microphone_device: AudioDevice | null }>("/api/capture/start", {
      method: "POST",
      body: JSON.stringify({}),
    }),
  stop: () => request<{ running: boolean }>("/api/capture/stop", { method: "POST" }),
  testOsc: () => request<{ queued: boolean }>("/api/osc/test", { method: "POST" }),
  asrCapabilities: () => request<AsrCapabilities>("/api/asr/capabilities"),
  asrModels: () => request<AsrModelRecord[]>("/api/asr/models"),
  apiProfiles: () => request<{ profiles: ApiProfileView[] }>("/api/asr/profiles"),
  createApiProfile: (profile: Omit<ApiProfile, "id"> & { api_key?: string }) =>
    request<ApiProfileView>("/api/asr/profiles", {
      method: "POST",
      body: JSON.stringify(profile),
    }),
  updateApiProfile: (profile: ApiProfile) =>
    request<ApiProfileView>(`/api/asr/profiles/${profile.id}`, {
      method: "PUT",
      body: JSON.stringify({
        name: profile.name,
        region: profile.region,
        workspace_id: profile.workspace_id,
        base_url: profile.base_url,
      }),
    }),
  deleteApiProfile: (profileId: string) =>
    request<{ deleted: boolean }>(`/api/asr/profiles/${profileId}`, { method: "DELETE" }),
  saveApiProfileCredential: (profileId: string, apiKey: string) =>
    request<ApiProfileView>(`/api/asr/profiles/${profileId}/credential`, {
      method: "PUT",
      body: JSON.stringify({ api_key: apiKey }),
    }),
  deleteApiProfileCredential: (profileId: string) =>
    request<ApiProfileView>(`/api/asr/profiles/${profileId}/credential`, { method: "DELETE" }),
  activateApiProfile: (provider: AsrApiProvider, profileId: string | null) =>
    request<{ profiles: ApiProfileView[] }>(`/api/asr/profiles/active/${provider}`, {
      method: "PUT",
      body: JSON.stringify({ profile_id: profileId }),
    }),
  testApiProfile: (profileId: string) =>
    request<{ ok: boolean }>(`/api/asr/profiles/${profileId}/test`, { method: "POST" }),
  apiProfileModels: (profileId: string) =>
    request<ApiModelCatalog>(`/api/asr/profiles/${profileId}/models`),
  translateSubtitle: (subtitleId: number) =>
    request<SubtitleTranslation>(`/api/subtitles/${subtitleId}/translation`, {
      method: "POST",
      body: JSON.stringify({}),
    }),
  previewTranslation: (text: string, sourceLanguage?: string | null, targetLanguage?: string) =>
    request<SubtitleTranslation>("/api/translations/preview", {
      method: "POST",
      body: JSON.stringify({
        text,
        source_language: sourceLanguage,
        target_language: targetLanguage,
      }),
    }),
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
  importDictionary: async (file: File, onProgress?: (progress: number) => void) => {
    const importId = dictionaryImportId();
    let finished = false;
    onProgress?.(0);
    const polling = (async () => {
      while (!finished) {
        try {
          const status = await request<DictionaryImportProgress>(
            `/api/dictionaries/import/${encodeURIComponent(importId)}`,
          );
          onProgress?.(Math.max(0, Math.min(1, status.progress)));
        } catch {
          // 上传完成且 Core 注册任务前会短暂返回 404，主请求仍负责错误处理。
        }
        await delay(100);
      }
    })();
    try {
      const response = await fetch(`${connection.httpUrl}/api/dictionaries/import`, {
        method: "POST",
        headers: requestHeaders({
          "Content-Type": "application/zip",
          "X-VRCS-Import-Id": importId,
        }),
        body: file,
      });
      if (!response.ok) {
        throw await apiErrorFromResponse(response);
      }
      onProgress?.(1);
      return (await response.json()) as DictionarySource;
    } finally {
      finished = true;
      await polling;
    }
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
