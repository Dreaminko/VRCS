import { defaultEnabledCapabilities, profileEnabledCapabilities, providerDefinition } from "./provider-catalog.ts";
import type {
  ApiAuthMode,
  ApiCapability,
  ApiProfileInput,
  ApiProfileView,
  HttpHeaderConfig,
  ProviderConnectionField,
  ProviderDefinition,
} from "./types";

export interface ApiProfileEditorDraft {
  id?: string;
  name: string;
  provider: string;
  enabled_capabilities: ApiCapability[];
  base_url: string;
  api_key: string;
  preset_id?: string;
  auth_mode: ApiAuthMode;
  timeout_ms: number;
  headers: HttpHeaderConfig[];
  fields: Record<string, string | number | boolean>;
}

export function createApiProfileDraft(
  definitions: ProviderDefinition[],
  providerId?: string,
): ApiProfileEditorDraft {
  const definition = providerDefinition(definitions, providerId ?? definitions[0]?.id ?? "");
  return {
    name: "",
    provider: definition?.id ?? providerId ?? "",
    enabled_capabilities: defaultEnabledCapabilities(definition),
    base_url: definition?.connection.base_url.default ?? "",
    api_key: "",
    auth_mode: definition?.connection.default_auth_mode ?? "bearer",
    timeout_ms: definition?.id === "qwen_local" ? 30_000 : 8000,
    headers: [],
    fields: Object.fromEntries(
      (definition?.connection.fields ?? []).map((field) => [field.id, field.default ?? defaultFieldValue(field)]),
    ),
  };
}

export function apiProfileDraftFromView(
  profile: ApiProfileView,
  definitions: ProviderDefinition[],
): ApiProfileEditorDraft {
  const definition = providerDefinition(definitions, profile.provider);
  return {
    id: profile.id,
    name: profile.name,
    provider: profile.provider,
    enabled_capabilities: profileEnabledCapabilities(profile),
    base_url: profile.base_url ?? definition?.connection.base_url.default ?? "",
    api_key: "",
    auth_mode: profile.auth_mode ?? definition?.connection.default_auth_mode ?? "bearer",
    preset_id: profile.preset_id,
    timeout_ms: profile.timeout_ms ?? 8000,
    headers: profile.headers ?? [],
    fields: Object.fromEntries(
      (definition?.connection.fields ?? []).map((field) => {
        const value = profile[field.id];
        return [field.id, isFieldValue(value) ? value : field.default ?? defaultFieldValue(field)];
      }),
    ),
  };
}

export function apiProfileFromEditorDraft(draft: ApiProfileEditorDraft): ApiProfileInput {
  return {
    name: draft.name.trim(),
    provider: draft.provider,
    enabled_capabilities: draft.enabled_capabilities,
    preset_id: draft.preset_id,
    ...(draft.base_url.trim() ? { base_url: draft.base_url.trim() } : {}),
    auth_mode: draft.auth_mode,
    timeout_ms: draft.timeout_ms,
    headers: draft.headers
      .map((header) => ({ name: header.name.trim(), value: header.value }))
      .filter((header) => header.name),
    region: typeof draft.fields.region === "string" ? draft.fields.region.trim() : undefined,
    workspace_id: typeof draft.fields.workspace_id === "string" ? draft.fields.workspace_id.trim() : undefined,
  };
}

export function apiProfileDraftErrors(
  draft: ApiProfileEditorDraft,
  definition: ProviderDefinition | undefined,
  credentialConfigured: boolean,
  requireCredential: boolean,
): Record<string, string> {
  const errors: Record<string, string> = {};
  if (!definition || !draft.provider) errors.provider = "provider";
  if (draft.enabled_capabilities.length === 0) errors.enabled_capabilities = "capabilities";
  if (definition?.connection.base_url.mode === "editable") {
    try {
      const url = new URL(draft.base_url.trim());
      const host = url.hostname.replace(/^\[|\]$/g, "").toLowerCase();
      const loopback = host === "localhost" || host === "::1" || /^127(?:\.\d{1,3}){3}$/.test(host);
      if (!url.host || !["http:", "https:"].includes(url.protocol)
        || url.username || url.password || draft.base_url.includes("?") || draft.base_url.includes("#") || draft.base_url.trim().length > 2048) {
        errors.base_url = "baseUrl";
      } else if (draft.auth_mode === "bearer" && url.protocol === "http:" && !loopback) {
        errors.base_url = "httpsRequired";
      }
    } catch {
      errors.base_url = "baseUrl";
    }
  }
  if (requireCredential && draft.auth_mode !== "none" && !credentialConfigured && !draft.api_key.trim()) {
    errors.api_key = "apiKey";
  }
  if (!Number.isInteger(draft.timeout_ms) || draft.timeout_ms < 1000 || draft.timeout_ms > 120000) {
    errors.timeout_ms = "timeout";
  }
  for (const field of definition?.connection.fields ?? []) {
    if (!field.required) continue;
    const value = draft.fields[field.id];
    if (!(typeof value === "boolean" ? value : String(value ?? "").trim().length > 0)) {
      errors[field.id] = "required";
    }
  }
  if (definition?.category === "custom_protocol") {
    const blockedHeaders = new Set([
      "authorization", "proxy-authorization", "cookie", "set-cookie", "x-api-key", "x-goog-api-key",
      "content-type", "accept", "user-agent", "host", "content-length",
    ]);
    const headers = draft.headers.filter((header) => header.name.trim() || header.value);
    if (headers.length > 16) errors.headers = "headerCount";
    const names = new Set<string>();
    for (const header of headers) {
      const name = header.name.trim().toLowerCase();
      if (!/^[!#$%&'*+.^_`|~0-9a-z-]+$/.test(name) || name.length > 128 || blockedHeaders.has(name)) {
        errors.headers = "headerName";
      } else if (names.has(name)) {
        errors.headers = "headerDuplicate";
      }
      names.add(name);
      if (new TextEncoder().encode(header.value).length > 2048 || /[\x00-\x08\x0a-\x1f\x7f]/.test(header.value)) {
        errors.headers = "headerValue";
      }
    }
  }
  return errors;
}

export function toggleDraftCapability(
  draft: ApiProfileEditorDraft,
  capability: ApiCapability,
): ApiProfileEditorDraft {
  const enabled = draft.enabled_capabilities.includes(capability);
  return {
    ...draft,
    enabled_capabilities: enabled
      ? draft.enabled_capabilities.filter((item) => item !== capability)
      : [...draft.enabled_capabilities, capability],
  };
}

function defaultFieldValue(field: ProviderConnectionField): string | number | boolean {
  if (field.type === "boolean") return false;
  if (field.type === "number") return field.min ?? 0;
  return field.options?.[0]?.value ?? "";
}

function isFieldValue(value: unknown): value is string | number | boolean {
  return typeof value === "string" || typeof value === "number" || typeof value === "boolean";
}
