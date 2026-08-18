import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { coreApi } from "../../api";
import { localizedError } from "../../app/app-utils";
import { explanationLanguageForUiLocale } from "../../learning";
import type { ApiProfileView, LearningLevel } from "../../types";

const LEARNING_PREFERENCES_KEY = "vrcs.learning.preferences.v1";

export interface LearningPreferences {
  profileId: string;
  model: string;
  explanationLanguage: string;
  explanationLevel: LearningLevel;
}

const DEFAULT_PREFERENCES: LearningPreferences = {
  profileId: "",
  model: "",
  explanationLanguage: "en-US",
  explanationLevel: "beginner",
};

export function useLearningAiConfiguration(active: boolean) {
  const { t, i18n } = useTranslation();
  const [error, setError] = useState("");
  const [profiles, setProfiles] = useState<ApiProfileView[]>([]);
  const [profilesLoading, setProfilesLoading] = useState(false);
  const [models, setModels] = useState<string[]>([]);
  const [modelsLoading, setModelsLoading] = useState(false);
  const [modelsError, setModelsError] = useState("");
  const [preferences, setPreferencesState] = useState<LearningPreferences>(() => (
    readLearningPreferences(i18n.resolvedLanguage)
  ));
  const profileRequestRef = useRef(0);
  const modelRequestRef = useRef(0);

  const selectedProfile = useMemo(
    () => profiles.find((profile) => profile.id === preferences.profileId),
    [preferences.profileId, profiles],
  );

  const setPreferences = useCallback((
    next: LearningPreferences | ((current: LearningPreferences) => LearningPreferences),
  ) => {
    setPreferencesState((current) => {
      const value = typeof next === "function" ? next(current) : next;
      writeLearningPreferences(value);
      return value;
    });
  }, []);

  const loadProfiles = useCallback(async () => {
    const requestId = ++profileRequestRef.current;
    setProfilesLoading(true);
    setError("");
    try {
      const response = await coreApi.apiProfiles();
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
    setPreferencesState(readLearningPreferences(i18n.resolvedLanguage));
  }, [active, i18n.resolvedLanguage]);

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
    void coreApi.apiProfileModels(selectedProfile.id)
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

function readLearningPreferences(uiLocale?: string): LearningPreferences {
  const explanationLanguage = explanationLanguageForUiLocale(uiLocale);
  if (typeof window === "undefined") return { ...DEFAULT_PREFERENCES, explanationLanguage };
  try {
    const parsed = JSON.parse(
      window.localStorage.getItem(LEARNING_PREFERENCES_KEY) ?? "null",
    ) as Partial<LearningPreferences> | null;
    if (!parsed) return { ...DEFAULT_PREFERENCES, explanationLanguage };
    return {
      profileId: typeof parsed.profileId === "string" ? parsed.profileId : "",
      model: typeof parsed.model === "string" ? parsed.model : "",
      explanationLanguage: normalizeExplanationLanguage(
        parsed.explanationLanguage,
        explanationLanguage,
      ),
      explanationLevel: normalizeLearningLevel(parsed.explanationLevel),
    };
  } catch {
    return { ...DEFAULT_PREFERENCES, explanationLanguage };
  }
}

function normalizeExplanationLanguage(value: unknown, fallback: string): string {
  return value === "zh-CN" || value === "ja-JP" || value === "en-US" ? value : fallback;
}

function normalizeLearningLevel(value: unknown): LearningLevel {
  if (value === "beginner" || value === "intermediate" || value === "advanced") return value;
  if (value === "brief") return "beginner";
  if (value === "standard") return "intermediate";
  if (value === "detailed") return "advanced";
  return DEFAULT_PREFERENCES.explanationLevel;
}

function writeLearningPreferences(preferences: LearningPreferences): void {
  if (typeof window === "undefined") return;
  window.localStorage.setItem(LEARNING_PREFERENCES_KEY, JSON.stringify(preferences));
}
