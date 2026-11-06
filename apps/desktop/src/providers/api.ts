import { request } from "../core-client/transport";
import type {
  ApiCapability,
  ApiModelCatalog,
  ApiProfile,
  ApiProfileInput,
  ApiProfileUsage,
  ApiProfileView,
  QwenModelRecord,
  QwenRuntimeStatus,
  ConnectionDiagnostic,
  ProviderDefinition,
} from "./types";

export interface ModelCatalogRequest {
  refresh?: boolean;
  cached_only?: boolean;
}

function modelCatalogQuery(options?: ModelCatalogRequest): string {
  return options?.refresh ? "?refresh=true" : options?.cached_only ? "?cached_only=true" : "";
}

function profileUpdateBody(profile: ApiProfile, apiKey?: string): string {
  return JSON.stringify({
    name: profile.name,
    region: profile.region ?? null,
    workspace_id: profile.workspace_id ?? null,
    base_url: profile.base_url ?? null,
    enabled_capabilities: profile.enabled_capabilities,
    preset_id: profile.preset_id ?? null,
    auth_mode: profile.auth_mode,
    is_local: profile.is_local,
    timeout_ms: profile.timeout_ms,
    headers: profile.headers,
    api_key: profile.auth_mode === "none" ? undefined : apiKey?.trim() || undefined,
  });
}

export const providersApi = {
  apiProfiles: () => request<{ profiles: ApiProfileView[] }>("/api/profile"),
  providers: () => request<{ providers: ProviderDefinition[] }>("/api/providers"),
  createApiProfile: (profile: ApiProfileInput & { api_key?: string }) => request<ApiProfileView>(
    "/api/profile",
    { method: "POST", body: JSON.stringify(profile) },
  ),
  updateApiProfile: (profile: ApiProfile, apiKey?: string) => request<ApiProfileView>(
    `/api/profile/${profile.id}`,
    { method: "PUT", body: profileUpdateBody(profile, apiKey) },
  ),
  previewApiProfileUpdate: (profile: ApiProfile, apiKey?: string) => request<{ side_effects: ApiProfileUsage[] }>(
    `/api/profile/${profile.id}?dry_run=true`,
    { method: "PUT", body: profileUpdateBody(profile, apiKey) },
  ),
  previewApiProfileDeletion: (profileId: string) => request<{ side_effects: ApiProfileUsage[] }>(
    `/api/profile/${profileId}?dry_run=true`,
    { method: "DELETE" },
  ),
  deleteApiProfile: (profileId: string) => request<{ deleted: boolean; side_effects?: ApiProfileUsage[] }>(
    `/api/profile/${profileId}`,
    { method: "DELETE" },
  ),
  deleteApiProfileCredential: (profileId: string) => request<ApiProfileView>(
    `/api/profile/${profileId}/credential`,
    { method: "DELETE" },
  ),
  activateAsrProfile: (profileId: string, serviceId: string) => request<{
    profiles: ApiProfileView[];
  }>("/api/asr/active", {
    method: "PUT",
    body: JSON.stringify({ profile_id: profileId, service_id: serviceId }),
  }),
  testApiProfile: (
    profileId: string,
    capability: ApiCapability,
    serviceId?: string,
    model?: string,
    options?: { signal?: AbortSignal; timeoutMs?: number },
  ) => {
    const query = new URLSearchParams({ capability });
    if (serviceId) query.set("service_id", serviceId);
    if (model?.trim()) query.set("model", model.trim());
    return request<ConnectionDiagnostic>(
      `/api/profile/${profileId}/test?${query.toString()}`,
      { method: "POST", signal: options?.signal, timeoutMs: options?.timeoutMs ?? 125_000 },
    );
  },
  apiProfileModels: (profileId: string, options?: ModelCatalogRequest) => request<ApiModelCatalog>(
    `/api/profile/${profileId}/models${modelCatalogQuery(options)}`,
  ),
  recognitionServiceModels: (profileId: string, serviceId: string, options?: ModelCatalogRequest) => request<ApiModelCatalog>(
    `/api/profile/${profileId}/services/${serviceId}/models${modelCatalogQuery(options)}`,
  ),
  qwenModels: () => request<QwenModelRecord[]>("/api/asr/local-models/qwen"),
  qwenRuntime: () => request<QwenRuntimeStatus>("/api/asr/local-models/qwen/runtime"),
  downloadQwenRuntime: () => request<{ started: boolean }>(
    "/api/asr/local-models/qwen/runtime/download",
    { method: "POST" },
  ),
  cancelQwenRuntimeDownload: () => request<{ cancelled: boolean }>(
    "/api/asr/local-models/qwen/runtime/cancel",
    { method: "POST" },
  ),
  downloadQwenModel: (id: string) => request<QwenModelRecord>(
    `/api/asr/local-models/qwen/${encodeURIComponent(id)}/download`,
    { method: "POST" },
  ),
  cancelQwenDownload: (id: string) => request<{ cancelled: boolean }>(
    `/api/asr/local-models/qwen/${encodeURIComponent(id)}/cancel`,
    { method: "POST" },
  ),
  verifyQwenModel: (id: string) => request<QwenModelRecord>(
    `/api/asr/local-models/qwen/${encodeURIComponent(id)}/verify`,
    { method: "POST" },
  ),
  deleteQwenModel: (id: string) => request<{ deleted: boolean }>(
    `/api/asr/local-models/qwen/${encodeURIComponent(id)}`,
    { method: "DELETE" },
  ),
};
