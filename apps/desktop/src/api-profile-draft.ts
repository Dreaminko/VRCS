import { defaultEnabledCapabilities, profileEnabledCapabilities, providerDefinition } from "./provider-catalog";
import type {
  ApiAuthMode,
  ApiCapability,
  ApiProfileInput,
  ApiProfileView,
  HttpHeaderConfig,
  ProviderConnectionField,
  ProviderDefinition,
} from "./providers/types";

export interface ApiProfileEditorDraft {
  id?: string;
  name: string;
  provider: string;
  enabled_capabilities: ApiCapability[];
  base_url: string;
  api_key: string;
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
    timeout_ms: 8000,
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
    ...(draft.base_url.trim() ? { base_url: draft.base_url.trim() } : {}),
    auth_mode: draft.auth_mode,
    timeout_ms: draft.timeout_ms,
    headers: draft.headers
      .map((header) => ({ name: header.name.trim(), value: header.value }))
      .filter((header) => header.name),
    ...Object.fromEntries(Object.entries(draft.fields).map(([key, value]) => [
      key,
      typeof value === "string" ? value.trim() : value,
    ])),
  };
}

export function apiProfileDraftCanSave(
  draft: ApiProfileEditorDraft,
  definition: ProviderDefinition | undefined,
  credentialConfigured: boolean,
  requireCredential: boolean,
): boolean {
  if (!draft.provider || !draft.name.trim() || draft.enabled_capabilities.length === 0) return false;
  if (definition?.connection.base_url.mode === "editable" && !draft.base_url.trim()) return false;
  if (requireCredential && draft.auth_mode !== "none" && !credentialConfigured && !draft.api_key.trim()) return false;
  if (draft.timeout_ms < 1000 || draft.timeout_ms > 120000) return false;
  return (definition?.connection.fields ?? []).every((field) => {
    if (!field.required) return true;
    const value = draft.fields[field.id];
    return typeof value === "boolean" ? value : String(value ?? "").trim().length > 0;
  });
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
