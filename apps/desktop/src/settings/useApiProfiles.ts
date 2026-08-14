import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { coreApi } from "../api";
import { supportsLlmModels } from "../api-profile-purpose";
import { localizedError } from "../app-utils";
import type {
  ApiProfile,
  ApiProfilePurpose,
  ApiProfileView,
  ConnectionDiagnostic,
  ProviderDefinition,
  AsrApiProvider,
  AsrSettings,
} from "../types";

export interface ApiModelCatalogState {
  models: string[];
  loading: boolean;
  error: string;
}

export function useApiProfiles(onRefreshSettings: () => Promise<void>) {
  const { t } = useTranslation();
  const [profiles, setProfiles] = useState<ApiProfileView[]>([]);
  const [providerDefinitions, setProviderDefinitions] = useState<ProviderDefinition[]>([]);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState<string | null>(null);
  const [message, setMessage] = useState("");
  const [modelCatalogs, setModelCatalogs] = useState<Record<string, ApiModelCatalogState>>({});
  const [diagnostics, setDiagnostics] = useState<Record<string, ConnectionDiagnostic>>({});
  const requestedModels = useRef(new Set<string>());

  const load = useCallback(async () => {
    const [profileResponse, providerResponse] = await Promise.all([
      coreApi.apiProfiles(),
      coreApi.providers(),
    ]);
    setProfiles(profileResponse.profiles);
    setProviderDefinitions(providerResponse.providers);
  }, []);

  useEffect(() => {
    void load()
      .catch((reason) => setMessage(localizedError(reason, t, "errors.apiProfiles.operation")))
      .finally(() => setLoading(false));
  }, [load]);

  const refreshModels = useCallback(async (profileId: string, force = true) => {
    if (!force && requestedModels.current.has(profileId)) return false;
    requestedModels.current.add(profileId);
    setModelCatalogs((current) => ({
      ...current,
      [profileId]: {
        models: current[profileId]?.models ?? [],
        loading: true,
        error: "",
      },
    }));
    try {
      const response = await coreApi.apiProfileModels(profileId);
      setModelCatalogs((current) => ({
        ...current,
        [profileId]: { models: response.models, loading: false, error: "" },
      }));
      return true;
    } catch (reason) {
      setModelCatalogs((current) => ({
        ...current,
        [profileId]: {
          models: current[profileId]?.models ?? [],
          loading: false,
          error: localizedError(reason, t, "errors.apiProfiles.models"),
        },
      }));
      return false;
    }
  }, [t]);

  useEffect(() => {
    for (const profile of profiles) {
      if (
        supportsLlmModels(profile)
        && (!profile.capabilities.requires_api_key || profile.credential.configured)
      ) {
        void refreshModels(profile.id, false);
      }
    }
  }, [profiles, refreshModels]);

  const invalidateModels = (profileId: string) => {
    requestedModels.current.delete(profileId);
    setModelCatalogs((current) => {
      const next = { ...current };
      delete next[profileId];
      return next;
    });
  };

  const run = async <T,>(
    busyKey: string,
    action: () => Promise<T>,
    successKey: string,
    refreshSettings: boolean,
  ): Promise<T | null> => {
    setBusy(busyKey);
    setMessage("");
    try {
      const result = await action();
      if (refreshSettings) await onRefreshSettings();
      await load();
      setMessage(t(successKey));
      return result;
    } catch (reason) {
      setMessage(localizedError(reason, t, "errors.apiProfiles.operation"));
      try {
        await load();
        if (refreshSettings) await onRefreshSettings();
      } catch {
        // 原始操作错误更有诊断价值。
      }
      return null;
    } finally {
      setBusy(null);
    }
  };

  return {
    profiles,
    providerDefinitions,
    loading,
    busy,
    message,
    modelCatalogs,
    diagnostics,
    refreshModels: (profileId: string) => refreshModels(profileId, true),
    create: (profile: Omit<ApiProfile, "id">, apiKey: string) => run(
      "create",
      () => coreApi.createApiProfile({
        ...profile,
        api_key: apiKey.trim() || undefined,
      }),
      "settings.apiManagement.profileCreated",
      true,
    ),
    update: (profile: ApiProfile, apiKey: string) => {
      invalidateModels(profile.id);
      return run(
        profile.id,
        async () => {
          let saved = await coreApi.updateApiProfile(profile);
          if (apiKey.trim()) saved = await coreApi.saveApiProfileCredential(profile.id, apiKey);
          return saved;
        },
        "settings.apiManagement.profileSaved",
        true,
      );
    },
    activate: (provider: AsrApiProvider, profileId: string) => run(
      profileId,
      () => coreApi.activateApiProfile(provider, profileId),
      "settings.apiManagement.profileActivated",
      true,
    ),
    test: async (
      profileId: string,
      capability: Extract<ApiProfilePurpose, "asr" | "llm">,
      backend?: Exclude<AsrSettings["backend"], "local_whisper">,
      model?: string,
    ) => {
      const result = await run(
        profileId,
        () => coreApi.testApiProfile(profileId, capability, backend, model),
        "settings.apiManagement.connectionSucceeded",
        false,
      );
      if (result) {
        setDiagnostics((current) => ({ ...current, [profileId]: result }));
        if (!result.ok) setMessage(t("settings.apiManagement.connectionFailed"));
      }
      return result;
    },
    removeCredential: (profileId: string) => run(
      profileId,
      () => coreApi.deleteApiProfileCredential(profileId),
      "settings.apiManagement.credentialRemoved",
      false,
    ),
    remove: (profileId: string) => run(
      profileId,
      () => coreApi.deleteApiProfile(profileId),
      "settings.apiManagement.profileDeleted",
      true,
    ),
  };
}
