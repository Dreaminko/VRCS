import type {
  ApiCapability,
  ApiProfile,
  ApiProfileView,
  ProviderCategory,
  ProviderConnectionField,
  ProviderDefinition,
  ProviderServiceDefinition,
} from "./types";

export const PROVIDER_CATEGORY_ORDER: readonly ProviderCategory[] = [
  "cloud_provider",
  "local_service",
  "custom_protocol",
];

export function providerDefinition(
  definitions: ProviderDefinition[],
  providerId: string,
): ProviderDefinition | undefined {
  return definitions.find((definition) => definition.id === providerId);
}

export function providerServicesWithCapability(
  definition: ProviderDefinition | undefined,
  capability: ApiCapability,
): ProviderServiceDefinition[] {
  return definition?.services.filter((service) => service.capabilities.includes(capability)) ?? [];
}

export function providerCapabilities(definition: ProviderDefinition | undefined): ApiCapability[] {
  return [...new Set(definition?.services.flatMap((service) => service.capabilities) ?? [])];
}

export function defaultEnabledCapabilities(definition: ProviderDefinition | undefined): ApiCapability[] {
  const available = providerCapabilities(definition);
  if (!definition) return [];
  if (definition.id === "groq") {
    const preferred: ApiCapability[] = ["text_generation", "text_translation", "speech_to_text"];
    return preferred.filter((capability) => available.includes(capability));
  }
  if (definition.category === "custom_protocol") {
    return available.filter((capability) => capability !== "speech_to_text");
  }
  return available;
}

export function profileHasCapability(
  profile: ApiProfile,
  capability: ApiCapability,
): boolean {
  return profile.enabled_capabilities.includes(capability);
}

export function profileEnabledCapabilities(profile: ApiProfileView): ApiCapability[] {
  if (profile.enabled_capabilities.length > 0) return profile.enabled_capabilities;
  const capabilities: ApiCapability[] = [];
  if (profile.capabilities.supports_text_generation) capabilities.push("text_generation");
  if (profile.capabilities.supports_translation) capabilities.push("text_translation");
  if (profile.capabilities.supports_asr) capabilities.push("speech_to_text");
  return capabilities;
}

export function profileSupportsCapability(
  profile: ApiProfileView,
  capability: ApiCapability,
): boolean {
  if (profileEnabledCapabilities(profile).includes(capability)) return true;
  if (capability === "speech_to_text") return profile.capabilities.supports_asr;
  if (capability === "text_translation") return profile.capabilities.supports_translation;
  return profile.capabilities.supports_text_generation;
}

export function connectionFieldValue(
  profile: ApiProfile,
  field: ProviderConnectionField,
): string | number | boolean {
  const value = profile[field.id];
  if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") {
    return value;
  }
  return field.default ?? "";
}

export function providerDetail(profile: ApiProfile, definition: ProviderDefinition | undefined): string {
  const parts: string[] = [];
  if (definition?.connection.base_url.mode === "editable" && profile.base_url) parts.push(profile.base_url);
  for (const field of definition?.connection.fields ?? []) {
    const value = connectionFieldValue(profile, field);
    if (value === "" || value === false) continue;
    const option = field.options?.find((item) => item.value === String(value));
    parts.push(option?.label ?? String(value));
  }
  return parts.join(" · ");
}

export function groupedProviderOptions(
  definitions: ProviderDefinition[],
  categoryLabels: Record<ProviderCategory, string>,
) {
  return [...definitions]
    .sort((left, right) => {
      const category = PROVIDER_CATEGORY_ORDER.indexOf(left.category)
        - PROVIDER_CATEGORY_ORDER.indexOf(right.category);
      return category || left.display_name.localeCompare(right.display_name);
    })
    .map((definition) => ({
      value: definition.id,
      label: definition.display_name,
      group: categoryLabels[definition.category],
    }));
}
