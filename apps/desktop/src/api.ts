import type {
  AnkiCardInput,
  AnkiStatus,
  ApiProfile,
  ApiProfileView,
  ConnectionDiagnostic,
  ProviderDefinition,
  ApiModelCatalog,
  AsrApiProvider,
  AsrCapabilities,
  AsrModelRecord,
  AudioDevice,
  ChatboxComposeInput,
  ChatboxMessage,
  ChatboxPreview,
  CredentialStatus,
  DictionaryEntry,
  DictionaryImportProgress,
  DictionarySource,
  DatabaseStorageStats,
  ExternalApiRuntimeStatus,
  Health,
  GlossarySourceStatus,
  LearningAnalysisInput,
  LearningDraftInput,
  LearningItem,
  LearningItemCreateInput,
  LearningItemPatchInput,
  LearningItemStatus,
  Settings,
  Subtitle,
  SubtitleTranslation,
  TranslationPromptPreview,
  TranslationPromptSettings,
  VrcxRuntimeStatus,
} from "./types";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { apiErrorFromResponse } from "./api-error";
import type {
  ConversationCatalog,
  ConversationIcon,
} from "./conversations";
import type { ConversationSubtitlePage } from "./subtitle-stream";

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
  subtitles: ({ limit = 100, beforeId }: { limit?: number; beforeId?: number } = {}) => {
    const params = new URLSearchParams({ limit: String(limit) });
    if (beforeId !== undefined) params.set("before_id", String(beforeId));
    return request<Subtitle[]>(`/api/subtitles?${params}`);
  },
  conversations: () => request<ConversationCatalog>("/api/conversations"),
  createConversation: () => request<ConversationCatalog>("/api/conversations", {
    method: "POST",
    body: JSON.stringify({}),
  }),
  updateConversation: (
    conversationId: string,
    input: { custom_title?: string | null; icon?: ConversationIcon | null },
  ) => request<ConversationCatalog>(
    `/api/conversations/${encodeURIComponent(conversationId)}`,
    {
      method: "PATCH",
      body: JSON.stringify(input),
    },
  ),
  deleteConversation: (conversationId: string) => request<ConversationCatalog>(
    `/api/conversations/${encodeURIComponent(conversationId)}`,
    { method: "DELETE" },
  ),
  conversationSubtitles: (
    conversationId: string,
    {
      limit = 100,
      beforeId,
      signal,
    }: { limit?: number; beforeId?: number; signal?: AbortSignal } = {},
  ) => {
    const params = new URLSearchParams({ limit: String(limit) });
    if (beforeId !== undefined) params.set("before_id", String(beforeId));
    return request<ConversationSubtitlePage>(
      `/api/conversations/${encodeURIComponent(conversationId)}/subtitles?${params}`,
      { signal },
    );
  },
  storageStats: () => request<DatabaseStorageStats>("/api/storage/stats"),
  clearSubtitleHistory: () => request<DatabaseStorageStats>("/api/subtitles", {
    method: "DELETE",
  }),
  deleteSubtitleRange: (startedAt: string, endedAt?: string) =>
    request<{ deleted: number }>("/api/subtitles/range", {
      method: "DELETE",
      body: JSON.stringify({
        started_at: startedAt,
        ended_at: endedAt,
      }),
    }),
  devices: () => request<AudioDevice[]>("/api/audio/devices"),
  settings: () => settingsRequest(),
  saveSettings: (settings: Settings) =>
    settingsRequest({
      method: "PUT",
      body: JSON.stringify(settings),
    }),
  externalApiTokenStatus: () =>
    request<CredentialStatus>("/api/external-api/token"),
  externalApiRuntimeStatus: () =>
    request<ExternalApiRuntimeStatus>("/api/external-api/status"),
  saveExternalApiToken: (token: string) =>
    request<CredentialStatus>("/api/external-api/token", {
      method: "PUT",
      body: JSON.stringify({ token }),
    }),
  deleteExternalApiToken: () =>
    request<CredentialStatus>("/api/external-api/token", { method: "DELETE" }),
  vrcxTokenStatus: () => request<CredentialStatus>("/api/vrcx/token"),
  vrcxRuntimeStatus: () => request<VrcxRuntimeStatus>("/api/vrcx/status"),
  saveVrcxToken: (token: string) =>
    request<CredentialStatus>("/api/vrcx/token", {
      method: "PUT",
      body: JSON.stringify({ token }),
    }),
  deleteVrcxToken: () =>
    request<CredentialStatus>("/api/vrcx/token", { method: "DELETE" }),
  testVrcx: () =>
    request<VrcxRuntimeStatus>("/api/vrcx/test", { method: "POST" }),
  start: () =>
    request<{ running: boolean; device: AudioDevice | null; microphone_device: AudioDevice | null }>("/api/capture/start", {
      method: "POST",
      body: JSON.stringify({}),
    }),
  stop: () => request<{ running: boolean }>("/api/capture/stop", { method: "POST" }),
  startMicrophoneTest: () => request<{ running: boolean; device: AudioDevice }>(
    "/api/audio/microphone-test/start",
    { method: "POST" },
  ),
  stopMicrophoneTest: () => request<{ running: boolean }>(
    "/api/audio/microphone-test/stop",
    { method: "POST" },
  ),
  testOsc: () => request<{ queued: boolean }>("/api/osc/test", { method: "POST" }),
  previewChatbox: (input: ChatboxComposeInput) =>
    request<ChatboxPreview>("/api/chatbox/preview", {
      method: "POST",
      body: JSON.stringify(input),
    }),
  sendChatbox: (input: ChatboxComposeInput) =>
    request<ChatboxMessage>("/api/chatbox/messages", {
      method: "POST",
      body: JSON.stringify(input),
    }),
  asrCapabilities: () => request<AsrCapabilities>("/api/asr/capabilities"),
  asrModels: () => request<AsrModelRecord[]>("/api/asr/models"),
  apiProfiles: () => request<{ profiles: ApiProfileView[] }>("/api/asr/profiles"),
  providers: () => request<{ providers: ProviderDefinition[] }>("/api/providers"),
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
        purpose: profile.purpose,
        preset_id: profile.preset_id,
        auth_mode: profile.auth_mode,
        is_local: profile.is_local,
        timeout_ms: profile.timeout_ms,
        headers: profile.headers,
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
  testApiProfile: (
    profileId: string,
    capability: "asr" | "llm",
    backend?: Exclude<Settings["asr"]["backend"], "local_whisper">,
    model?: string,
  ) => {
    const query = new URLSearchParams({ capability });
    if (backend) query.set("backend", backend);
    if (model?.trim()) query.set("model", model.trim());
    return request<ConnectionDiagnostic>(
      `/api/asr/profiles/${profileId}/test?${query.toString()}`,
      { method: "POST" },
    );
  },
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
  previewTranslationPrompt: (
    prompt: TranslationPromptSettings,
    sourceLanguage?: string | null,
    targetLanguage?: string,
  ) => request<TranslationPromptPreview>(
    "/api/translations/prompt-preview",
    {
      method: "POST",
      body: JSON.stringify({
        prompt,
        source_language: sourceLanguage,
        target_language: targetLanguage,
      }),
    },
  ),
  glossaryStatuses: () => request<GlossarySourceStatus[]>(
    "/api/translations/glossaries/status",
  ),
  refreshGlossary: (id: string) => request<unknown>(
    `/api/translations/glossaries/${encodeURIComponent(id)}/refresh`,
    { method: "POST" },
  ),
  downloadAsrModel: (model: AsrModelRecord["id"]) =>
    request<AsrModelRecord>(`/api/asr/models/${model}/download`, {
      method: "POST",
      body: JSON.stringify({}),
    }),
  deleteAsrModel: (model: AsrModelRecord["id"]) =>
    request<{ deleted: boolean }>(`/api/asr/models/${model}`, { method: "DELETE" }),
  learningItems: ({
    status,
    limit = 50,
    beforeId,
  }: {
    status?: LearningItemStatus;
    limit?: number;
    beforeId?: number;
  } = {}) => {
    const params = new URLSearchParams({ limit: String(limit) });
    if (status) params.set("status", status);
    if (beforeId !== undefined) params.set("before_id", String(beforeId));
    return request<LearningItem[]>(`/api/learning/items?${params}`);
  },
  learningCaptureKeys: () =>
    request<{ keys: string[] }>("/api/learning/capture-keys"),
  createLearningItem: (input: LearningItemCreateInput) =>
    request<LearningItem>("/api/learning/items", {
      method: "POST",
      body: JSON.stringify(input),
    }),
  updateLearningItem: (itemId: number, input: LearningItemPatchInput) =>
    request<LearningItem>(`/api/learning/items/${itemId}`, {
      method: "PATCH",
      body: JSON.stringify(input),
    }),
  archiveLearningItem: (itemId: number) =>
    request<LearningItem>(`/api/learning/items/${itemId}/archive`, { method: "POST" }),
  restoreLearningItem: (itemId: number) =>
    request<LearningItem>(`/api/learning/items/${itemId}/restore`, { method: "POST" }),
  deleteLearningItem: (itemId: number) =>
    request<{ deleted: boolean }>(`/api/learning/items/${itemId}`, { method: "DELETE" }),
  analyzeLearningItem: (itemId: number, input: LearningAnalysisInput) =>
    request<LearningItem>(`/api/learning/items/${itemId}/analysis`, {
      method: "POST",
      body: JSON.stringify(input),
    }),
  generateLearningDraft: (itemId: number, input: LearningDraftInput) =>
    request<LearningItem>(`/api/learning/items/${itemId}/draft`, {
      method: "POST",
      body: JSON.stringify(input),
    }),
  exportLearningItem: (itemId: number) =>
    request<LearningItem>(`/api/learning/items/${itemId}/export`, {
      method: "POST",
    }),
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
