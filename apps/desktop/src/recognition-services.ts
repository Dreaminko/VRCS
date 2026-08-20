import { profileSupportsCapability, providerDefinition, providerServicesWithCapability } from "./provider-catalog.ts";
import type {
  ApiProfileView,
  AsrSettings,
  ProviderDefinition,
  ProviderServiceDefinition,
} from "./types";

export const LOCAL_RECOGNITION_SOURCE = "local";

export function recognitionProfiles(profiles: ApiProfileView[]): ApiProfileView[] {
  return profiles.filter((profile) => profileSupportsCapability(profile, "speech_to_text"));
}

export function recognitionServicesForProfile(
  profile: ApiProfileView | undefined,
  definitions: ProviderDefinition[],
): ProviderServiceDefinition[] {
  if (!profile) return [];
  return providerServicesWithCapability(
    providerDefinition(definitions, profile.provider),
    "speech_to_text",
  );
}

export function recognitionSourceValue(asr: AsrSettings): string {
  return asr.backend === "local_whisper"
    ? LOCAL_RECOGNITION_SOURCE
    : asr.active_profile_id ?? "";
}

export function selectRecognitionProfile(
  asr: AsrSettings,
  source: string,
  profiles: ApiProfileView[],
  definitions: ProviderDefinition[],
): AsrSettings {
  if (source === LOCAL_RECOGNITION_SOURCE) {
    return { ...asr, backend: "local_whisper", active_profile_id: null };
  }

  const profile = recognitionProfiles(profiles).find((item) => item.id === source);
  const services = recognitionServicesForProfile(profile, definitions);
  if (!profile || services.length === 0) return asr;
  const service = services.find((item) => item.id === asr.backend) ?? services[0];
  return selectRecognitionService(
    { ...asr, active_profile_id: profile.id },
    service,
  );
}

export function selectRecognitionService(
  asr: AsrSettings,
  service: ProviderServiceDefinition,
): AsrSettings {
  const current = asr.service_settings[service.id];
  const model = current?.model && (
    service.model_listing
    || service.models.length === 0
    || service.models.includes(current.model)
  )
    ? current.model
    : service.models[0] ?? current?.model ?? "";
  return {
    ...asr,
    backend: service.id,
    service_settings: {
      ...asr.service_settings,
      [service.id]: {
        model,
        context: current?.context ?? "",
      },
    },
  };
}

export function updateRecognitionServiceSettings(
  asr: AsrSettings,
  serviceId: string,
  update: Partial<{ model: string; context: string }>,
): AsrSettings {
  const current = asr.service_settings[serviceId] ?? { model: "", context: "" };
  return {
    ...asr,
    service_settings: {
      ...asr.service_settings,
      [serviceId]: { ...current, ...update },
    },
  };
}

export function currentRecognitionProfile(
  asr: AsrSettings,
  profiles: ApiProfileView[],
): ApiProfileView | undefined {
  return profiles.find((profile) => profile.id === asr.active_profile_id);
}

export function currentRecognitionService(
  asr: AsrSettings,
  profile: ApiProfileView | undefined,
  definitions: ProviderDefinition[],
): ProviderServiceDefinition | undefined {
  return recognitionServicesForProfile(profile, definitions)
    .find((service) => service.id === asr.backend);
}

export function recognitionEngineLabel(
  asr: AsrSettings,
  profiles: ApiProfileView[],
  definitions: ProviderDefinition[],
): string {
  if (asr.backend === "local_whisper") return `Whisper ${capitalize(asr.local.model)}`;
  const profile = currentRecognitionProfile(asr, profiles);
  return currentRecognitionService(asr, profile, definitions)?.display_name || asr.backend;
}

function capitalize(value: string): string {
  return value.charAt(0).toUpperCase() + value.slice(1);
}
