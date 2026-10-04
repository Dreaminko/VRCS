import { request } from "../core-client/transport";
import type {
  ApiCapability,
  ApiModelCatalog,
  ApiProfile,
  ApiProfileInput,
  ApiProfileView,
  AsrCapabilities,
  AsrModelRecord,
  ConnectionDiagnostic,
  ProviderDefinition,
} from "./types";

export const providersApi = {
  asrCapabilities: () => request<AsrCapabilities>("/api/asr/capabilities"),
  asrModels: () => request<AsrModelRecord[]>("/api/asr/models"),
  apiProfiles: () => request<{ profiles: ApiProfileView[] }>("/api/asr/profiles"),
  providers: () => request<{ providers: ProviderDefinition[] }>("/api/providers"),
  createApiProfile: (profile: ApiProfileInput & { api_key?: string }) => request<ApiProfileView>(
    "/api/asr/profiles",
    { method: "POST", body: JSON.stringify(profile) },
  ),
  updateApiProfile: (profile: ApiProfile) => request<ApiProfileView>(
    `/api/asr/profiles/${profile.id}`,
    {
      method: "PUT",
      body: JSON.stringify({
        name: profile.name,
        region: profile.region,
        workspace_id: profile.workspace_id,
        base_url: profile.base_url,
        enabled_capabilities: profile.enabled_capabilities,
        preset_id: profile.preset_id,
        auth_mode: profile.auth_mode,
        is_local: profile.is_local,
        timeout_ms: profile.timeout_ms,
        headers: profile.headers,
        ...Object.fromEntries(Object.entries(profile).filter(([key]) => ![
          "id", "name", "provider", "enabled_capabilities", "region", "workspace_id", "base_url",
          "preset_id", "auth_mode", "is_local", "timeout_ms", "headers",
        ].includes(key))),
      }),
    },
  ),
  deleteApiProfile: (profileId: string) => request<{ deleted: boolean }>(
    `/api/asr/profiles/${profileId}`,
    { method: "DELETE" },
  ),
  saveApiProfileCredential: (profileId: string, apiKey: string) => request<ApiProfileView>(
    `/api/asr/profiles/${profileId}/credential`,
    { method: "PUT", body: JSON.stringify({ api_key: apiKey }) },
  ),
  deleteApiProfileCredential: (profileId: string) => request<ApiProfileView>(
    `/api/asr/profiles/${profileId}/credential`,
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
  ) => {
    const query = new URLSearchParams({ capability });
    if (serviceId) query.set("service_id", serviceId);
    if (model?.trim()) query.set("model", model.trim());
    return request<ConnectionDiagnostic>(
      `/api/asr/profiles/${profileId}/test?${query.toString()}`,
      { method: "POST" },
    );
  },
  apiProfileModels: (profileId: string) => request<ApiModelCatalog>(
    `/api/asr/profiles/${profileId}/models`,
  ),
  recognitionServiceModels: (profileId: string, serviceId: string) => request<ApiModelCatalog>(
    `/api/asr/profiles/${profileId}/services/${serviceId}/models`,
  ),
  downloadAsrModel: (model: AsrModelRecord["id"]) => request<AsrModelRecord>(
    `/api/asr/models/${model}/download`,
    { method: "POST", body: JSON.stringify({}) },
  ),
  deleteAsrModel: (model: AsrModelRecord["id"]) => request<{ deleted: boolean }>(
    `/api/asr/models/${model}`,
    { method: "DELETE" },
  ),
};
