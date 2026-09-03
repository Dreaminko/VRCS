import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { useTranslation } from "react-i18next";

import { providersApi } from "../../providers/api";
import { localizedError } from "../../app/app-utils";
import { explanationLanguageForUiLocale } from "../../learning";
import type { ApiProfileView } from "../../providers/types";
import {
  learningPreferencesSnapshot,
  normalizeLearningLevel,
  readLearningPreferences,
  subscribeLearningPreferences,
  updateLearningPreferences as setPreferences,
} from "../preferences";

export type { LearningPreferences } from "../preferences";

export function useLearningAiConfiguration(active: boolean) {
  const { t, i18n } = useTranslation();
  const [error, setError] = useState("");
  const [profiles, setProfiles] = useState<ApiProfileView[]>([]);
  const [profilesLoading, setProfilesLoading] = useState(false);
  const [models, setModels] = useState<string[]>([]);
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
        return { ...current, profileId: available[0]?.id ?? "", model: "" };
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
    void loadProfiles();
  }, [active, loadProfiles]);

  useEffect(() => {
    if (!active || !selectedProfile) {
      setModels([]);
      setModelsError("");
      setModelsLoading(false);
      return;
    }
    if (!selectedProfile.capabilities.supports_model_listing) {
      setModels([]);
      setModelsError("");
      setModelsLoading(false);
      return;
    }
    const requestId = ++modelRequestRef.current;
    setModelsLoading(true);
    setModelsError("");
    void providersApi.apiProfileModels(selectedProfile.id)
      .then((response) => {
        if (requestId === modelRequestRef.current) setModels(response.models);
      })
      .catch((reason) => {
        if (requestId === modelRequestRef.current) {
          setModels([]);
          setModelsError(localizedError(reason, t, "errors.learning.models"));
        }
      })
      .finally(() => {
        if (requestId === modelRequestRef.current) setModelsLoading(false);
      });
    return () => { modelRequestRef.current += 1; };
  }, [active, selectedProfile?.id, selectedProfile?.capabilities.supports_model_listing, t]);

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
    modelsLoading,
    modelsError,
    selectedProfile,
    preferences,
    explanationLanguagePreference: savedPreferences.explanationLanguage,
    setProfileId: (profileId: string) => setPreferences((current) => ({
      ...current,
      profileId,
      model: "",
    })),
    setModel: (model: string) => setPreferences((current) => ({ ...current, model })),
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
