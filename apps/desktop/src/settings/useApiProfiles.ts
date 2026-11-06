import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { providersApi } from "../providers/api";
import { localizedError } from "../app/app-utils";
import { ApiError } from "../core-client/api-error";
import { refreshApiProfileCatalog, useApiProfileCatalog } from "../providers/useApiProfileCatalog";
import { supportsLlmModels } from "../providers/api-profile-purpose";
import type {
  ApiCapability,
  ApiProfile,
  ApiProfileInput,
  ApiProfileUsage,
} from "../providers/types";

export interface ApiModelCatalogState {
  models: string[];
  loading: boolean;
  error: string;
}

export function useApiProfiles(onRefreshSettings: () => Promise<void>) {
  const { t } = useTranslation();
  const catalog = useApiProfileCatalog();
  const [busyKeys, setBusyKeys] = useState(new Set<string>());
  const operations = useRef(new Set<string>());
  const [message, setMessage] = useState("");
  const [editorError, setEditorError] = useState("");
  const [editorErrorField, setEditorErrorField] = useState<string>();
  const [rowMessages, setRowMessages] = useState<Record<string, string>>({});
  const [modelCatalogs, setModelCatalogs] = useState<Record<string, ApiModelCatalogState>>({});
  const profileVersions = useRef(new Map<string, number>());
  const testControllers = useRef(new Map<string, AbortController>());
  const [testing, setTesting] = useState(new Set<string>());

  useEffect(() => () => {
    testControllers.current.forEach((controller) => controller.abort());
  }, []);

  const refreshModels = useCallback(async (profileId: string, cachedOnly = false) => {
    const version = profileVersions.current.get(profileId);
    setModelCatalogs((current) => ({
      ...current,
      [profileId]: {
        models: current[profileId]?.models ?? [],
        loading: true,
        error: "",
      },
    }));
    try {
      const response = await providersApi.apiProfileModels(profileId, cachedOnly ? { cached_only: true } : { refresh: true });
      if (profileVersions.current.get(profileId) !== version) return false;
      setModelCatalogs((current) => ({
        ...current,
        [profileId]: { models: response.models, loading: false, error: "" },
      }));
      return true;
    } catch (reason) {
      if (profileVersions.current.get(profileId) !== version) return false;
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
    for (const profile of catalog.profiles) {
      if (supportsLlmModels(profile) && (!profile.status || profile.status === "ready")
        && !busyKeys.has(profile.id) && (!profile.capabilities.requires_api_key || profile.credential.configured) && !modelCatalogs[profile.id]) {
        void refreshModels(profile.id, true);
      }
    }
  }, [catalog.profiles, modelCatalogs, refreshModels, busyKeys]);

  const invalidateProfile = (profileId: string) => {
    profileVersions.current.set(profileId, (profileVersions.current.get(profileId) ?? 0) + 1);
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
    refreshProfiles = false,
    editorOperation = false,
  ): Promise<T | null> => {
    if (operations.current.has(busyKey)) return null;
    operations.current.add(busyKey);
    setBusyKeys(new Set(operations.current));
    setMessage("");
    if (editorOperation) { setEditorError(""); setEditorErrorField(undefined); }
    setRowMessages((current) => ({ ...current, [busyKey]: "" }));
    try {
      const result = await action();
      if (!successKey) return result;
      const refreshes: Promise<void>[] = [];
      if (refreshSettings) refreshes.push(Promise.resolve().then(onRefreshSettings));
      if (refreshProfiles || refreshSettings) refreshes.push(Promise.resolve().then(() => refreshApiProfileCatalog(true)));
      const refreshed = await Promise.allSettled(refreshes);
      const effects = (result as { side_effects?: ApiProfileUsage[] }).side_effects;
      const features = effects && [...new Set(effects.map((usage) => t(`settings.apiManagement.usages.${usage.kind}`)))].join(", ");
      const success = t(successKey)
        + (features ? ` ${t("settings.apiManagement.appliedEffects", { features })}` : "")
        + (refreshed.some((refresh) => refresh.status === "rejected") ? ` ${t("settings.apiManagement.refreshFailed")}` : "");
      if (busyKey === "create" || successKey === "settings.apiManagement.profileDeleted") setMessage(success);
      else setRowMessages((current) => ({ ...current, [busyKey]: success }));
      return result;
    } catch (reason) {
      const error = reason instanceof DOMException && reason.name === "AbortError"
        ? t("settings.apiManagement.testCancelled")
        : localizedError(reason, t, "errors.apiProfiles.operation");
      if (editorOperation) {
        setEditorError(error);
        setEditorErrorField(reason instanceof ApiError ? reason.field : undefined);
      }
      else setRowMessages((current) => ({ ...current, [busyKey]: error }));
      return null;
    } finally {
      operations.current.delete(busyKey);
      setBusyKeys(new Set(operations.current));
    }
  };

  return {
    profiles: catalog.profiles,
    providerDefinitions: catalog.providerDefinitions,
    loading: catalog.loading,
    busy: busyKeys.values().next().value ?? null,
    isBusy: (profileId: string) => busyKeys.has(profileId),
    isTesting: (profileId: string) => testing.has(profileId),
    cancelTest: (profileId: string) => testControllers.current.get(profileId)?.abort(),
    message: message || (catalog.error ? localizedError(catalog.error, t, "errors.apiProfiles.operation") : ""),
    editorError,
    editorErrorField,
    clearEditorError: () => { setEditorError(""); setEditorErrorField(undefined); },
    rowMessages,
    modelCatalogs,
    refreshModels: (profileId: string) => refreshModels(profileId),
    previewUpdate: (profile: ApiProfile, apiKey: string) => run(
      profile.id,
      () => providersApi.previewApiProfileUpdate(profile, apiKey),
      "", false, false, true,
    ),
    previewDeletion: (profileId: string) => run(
      profileId,
      () => providersApi.previewApiProfileDeletion(profileId),
      "", false,
    ),
    create: (profile: ApiProfileInput, apiKey: string) => run(
      "create",
      () => providersApi.createApiProfile({
        ...profile,
        api_key: profile.auth_mode === "none" ? undefined : apiKey.trim() || undefined,
      }),
      "settings.apiManagement.profileCreated",
      true,
      true,
      true,
    ),
    update: (profile: ApiProfile, apiKey: string) => {
      invalidateProfile(profile.id);
      return run(
        profile.id,
        () => providersApi.updateApiProfile(profile, apiKey),
        "settings.apiManagement.profileSaved",
        true,
        true,
        true,
      );
    },
    activate: (profileId: string, serviceId: string) => run(
      profileId,
      () => providersApi.activateAsrProfile(profileId, serviceId),
      "settings.apiManagement.profileActivated",
      true,
    ),
    test: async (
      profileId: string,
      capability: ApiCapability,
      serviceId?: string,
      model?: string,
    ) => {
      if (operations.current.has(profileId)) return null;
      const controller = new AbortController();
      testControllers.current.set(profileId, controller);
      setTesting(new Set(testControllers.current.keys()));
      const version = profileVersions.current.get(profileId);
      try {
        const profile = catalog.profiles.find((item) => item.id === profileId);
        const result = await run(
          profileId,
          () => providersApi.testApiProfile(profileId, capability, serviceId, model, {
            signal: controller.signal,
            timeoutMs: (profile?.timeout_ms ?? 8_000) * 3 + 5_000,
          }),
          "settings.apiManagement.connectionSucceeded",
          false,
        );
        if (controller.signal.aborted || profileVersions.current.get(profileId) !== version) return null;
        if (result && !result.ok) {
          const failure = result.checks?.find((check) => check.status === "failed" && check.code);
          const message = failure?.code
            ? localizedError(new ApiError({
              code: failure.code,
              detail: failure.detail ?? "",
              status: 502,
            }), t, "settings.apiManagement.connectionFailed")
            : t("settings.apiManagement.connectionFailed");
          setRowMessages((current) => ({ ...current, [profileId]: message }));
        }
        return result;
      } finally {
        testControllers.current.delete(profileId);
        setTesting(new Set(testControllers.current.keys()));
      }
    },
    removeCredential: (profileId: string) => run(
      profileId,
      () => providersApi.deleteApiProfileCredential(profileId),
      "settings.apiManagement.credentialRemoved",
      false,
      true,
      true,
    ),
    remove: (profileId: string) => run(
      profileId,
      () => providersApi.deleteApiProfile(profileId),
      "settings.apiManagement.profileDeleted",
      true,
      true,
    ),
  };
}
