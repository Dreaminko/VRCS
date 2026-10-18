import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { useTranslation } from "react-i18next";

import { providersApi } from "../../providers/api";
import { localizedError } from "../../app/app-utils";
import { explanationLanguageForUiLocale } from "../learning";
import type { ApiModelDetails, ApiProfileView } from "../../providers/types";
import { API_PROFILES_CHANGED_EVENT } from "../../settings/hooks/useApiProfileViews";
import { normalizeReasoningControlForModel } from "../../translation/translation-thinking";
import type { ReasoningSetting } from "../../translation/translation-thinking";
import {
  learningPreferencesSnapshot,
  normalizeLearningLevel,
  readLearningPreferences,
  subscribeLearningPreferences,
  updateLearningPreferences as setPreferences,
} from "../preferences";

export type { LearningPreferences } from "../preferences";

export function useLearningAiConfiguration(active: boolean, loadModelCatalog = active) {
  const { t, i18n } = useTranslation();
  const [error, setError] = useState("");
  const [profiles, setProfiles] = useState<ApiProfileView[]>([]);
  const [profilesLoading, setProfilesLoading] = useState(false);
  const [models, setModels] = useState<string[]>([]);
  const [modelDetails, setModelDetails] = useState<Record<string, ApiModelDetails>>({});
  const [modelsLoading, setModelsLoading] = useState(false);
  const [modelsError, setModelsError] = useState("");
  const serialized = useSyncExternalStore(subscribeLearningPreferences, learningPreferencesSnapshot);
  const savedPreferences = useMemo(() => readLearningPreferences(serialized), [serialized]);
  const preferences = useMemo(() => ({
    ...savedPreferences,
    explanationLanguage: savedPreferences.explanationLanguage === "ui"
      ? explanationLanguageForUiLocale(i18n.resolvedLanguage)
      : savedPreferences.explanationLanguage,
  }), [savedPreferences, i18n.resolvedLanguage]);
  const profileRequestRef = useRef(0);
  const modelRequestRef = useRef(0);

  const selectedProfile = useMemo(
    () => profiles.find((profile) => profile.id === preferences.profileId),
    [preferences.profileId, profiles],
  );

  const loadProfiles = useCallback(async () => {
    const requestId = ++profileRequestRef.current;
    setProfilesLoading(true);
    setError("");
    try {
      const response = await providersApi.apiProfiles();
      if (requestId !== profileRequestRef.current) return;
      const available = response.profiles.filter(
        (profile) => profile.capabilities.supports_text_generation === true,
      );
      setProfiles(available);
      setPreferences((current) => {
        if (available.some((profile) => profile.id === current.profileId)) return current;
        return { ...current, profileId: available[0]?.id ?? "", model: "", reasoningControl: null };
      });
    } catch (reason) {
      if (requestId === profileRequestRef.current) {
        setError(localizedError(reason, t, "errors.learning.profiles"));
      }
    } finally {
      if (requestId === profileRequestRef.current) setProfilesLoading(false);
    }
  }, [setPreferences, t]);

  useEffect(() => {
    if (!active) return;
    const reload = () => {
      void loadProfiles();
    };
    reload();
    window.addEventListener(API_PROFILES_CHANGED_EVENT, reload);
    return () => {
      window.removeEventListener(API_PROFILES_CHANGED_EVENT, reload);
    };
  }, [active, loadProfiles]);

  useEffect(() => {
    if (!active || !loadModelCatalog || !selectedProfile) {
      setModels([]);
      setModelDetails({});
      setModelsError("");
      setModelsLoading(false);
      return;
    }
    if (!selectedProfile.capabilities.supports_model_listing) {
      setModels([]);
      setModelDetails({});
      setModelsError("");
      setModelsLoading(false);
      return;
    }
    const requestId = ++modelRequestRef.current;
    setModelDetails({});
    setModelsLoading(true);
    setModelsError("");
    void providersApi.apiProfileModels(selectedProfile.id)
      .then((response) => {
        if (requestId === modelRequestRef.current) {
          setModels(response.models);
          setModelDetails(response.model_details ?? {});
        }
      })
      .catch((reason) => {
        if (requestId === modelRequestRef.current) {
          setModels([]);
          setModelDetails({});
          setModelsError(localizedError(reason, t, "errors.learning.models"));
        }
      })
      .finally(() => {
        if (requestId === modelRequestRef.current) setModelsLoading(false);
      });
    return () => { modelRequestRef.current += 1; };
  }, [active, loadModelCatalog, selectedProfile?.id, selectedProfile?.capabilities.supports_model_listing, t]);

  useEffect(() => () => {
    profileRequestRef.current += 1;
    modelRequestRef.current += 1;
  }, []);

  return {
    error,
    clearError: () => setError(""),
    profiles,
    profilesLoading,
    models,
    modelDetails,
    modelsLoading,
    modelsError,
    selectedProfile,
    preferences,
    explanationLanguagePreference: savedPreferences.explanationLanguage,
    setProfileId: (profileId: string) => setPreferences((current) => ({
      ...current,
      profileId,
      model: "",
      reasoningControl: null,
    })),
    setModel: (model: string) => setPreferences((current) => ({
      ...current,
      model,
      reasoningControl: normalizeReasoningControlForModel(
        selectedProfile?.provider, model, current.reasoningControl, modelDetails[model],
      ),
    })),
    setReasoningControl: (patch: {
      thinking_enabled?: boolean;
      reasoning_control?: ReasoningSetting | null;
    }) => setPreferences((current) => ({
      ...current,
      thinkingEnabled: patch.thinking_enabled ?? current.thinkingEnabled,
      reasoningControl: patch.reasoning_control === undefined
        ? current.reasoningControl : patch.reasoning_control,
    })),
    setExplanationLanguage: (explanationLanguage: string) => setPreferences((current) => ({
      ...current,
      explanationLanguage,
    })),
    setExplanationLevel: (explanationLevel: string) => setPreferences((current) => ({
      ...current,
      explanationLevel: normalizeLearningLevel(explanationLevel),
    })),
  };
}
