import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { coreApi } from "../../api";
import { supportsRecognition } from "../../api-profile-purpose";
import {
  apiProfileFromEditorDraft,
  createApiProfileDraft,
  type ApiProfileEditorDraft,
} from "../../settings/api/ApiProfileEditor";
import { useAsrModels } from "../../settings/hooks/useAsrModels";
import { useSettingsDraft } from "../../settings/hooks/useSettingsDraft";
import { selectRecognitionSource } from "../../settings/settings-derived";
import { asrSelectionError } from "../../settings/settings-validation";
import { useApiProfiles } from "../../settings/useApiProfiles";
import type { ApiProvider, AsrCapabilities, Settings } from "../../types";
import type { CloudBackend, RecognitionMode } from "../onboarding-types";

function backendProvider(backend: CloudBackend): ApiProvider {
  return backend === "openai_realtime" ? "openai" : "alibaba_cloud";
}

function initialCloudBackend(settings: Settings): CloudBackend {
  return settings.asr.backend === "openai_realtime"
    ? "openai_realtime"
    : settings.asr.backend === "fun_asr_realtime"
      ? "fun_asr_realtime"
      : "qwen_realtime";
}

export function useOnboardingRecognition({
  active,
  settings,
  asrCapabilities,
  modelStatus,
  onRefreshSettings,
  onModelsChanged,
  onSave,
  onFinish,
  clearMessage,
  showError,
  showInfo,
}: {
  active: boolean;
  settings: Settings;
  asrCapabilities: AsrCapabilities | null;
  modelStatus: string;
  onRefreshSettings: () => Promise<void>;
  onModelsChanged: () => Promise<void>;
  onSave: (settings: Settings) => Promise<Settings>;
  onFinish: () => Promise<void>;
  clearMessage: () => void;
  showError: (reason: unknown, fallbackKey?: string) => void;
  showInfo: (message: string) => void;
}) {
  const { t } = useTranslation();
  const [recognitionMode, setRecognitionMode] = useState<RecognitionMode>(
    settings.asr.backend === "local_whisper" ? "local" : "cloud",
  );
  const [cloudBackend, setCloudBackend] = useState<CloudBackend>(initialCloudBackend(settings));
  const [selectedProfileId, setSelectedProfileId] = useState("");
  const [testedProfileId, setTestedProfileId] = useState("");
  const [apiEditor, setApiEditor] = useState<ApiProfileEditorDraft | null>(null);
  const [busy, setBusy] = useState(false);

  const draftController = useSettingsDraft(settings, onSave);
  const asr = useAsrModels({
    active: active && recognitionMode === "local",
    settings,
    modelStatus,
    asrCapabilities,
    onModelsChanged,
    draftController,
  });
  const apiProfiles = useApiProfiles(onRefreshSettings);
  const provider = backendProvider(cloudBackend);
  const recognitionProfiles = useMemo(
    () => apiProfiles.profiles.filter(
      (profile) => profile.provider === provider && supportsRecognition(profile),
    ),
    [apiProfiles.profiles, provider],
  );
  const selectedProfile = recognitionProfiles.find((profile) => profile.id === selectedProfileId);
  const selectedModel = asr.managedModels.find(
    (model) => model.id === draftController.draft.asr.local.model,
  );
  const localSettingsError = asrSelectionError(
    draftController.draft,
    asrCapabilities,
    (key) => t(key),
  );
  const localReady = Boolean(
    selectedModel
    && ["downloaded", "loading", "ready"].includes(selectedModel.status)
    && !localSettingsError
    && draftController.saveState !== "error",
  );
  const saveBusy = draftController.saveState === "saving";

  useEffect(() => {
    if (recognitionProfiles.some((profile) => profile.id === selectedProfileId)) return;
    const activeId = provider === "openai"
      ? settings.asr.active_api_profiles.openai
      : settings.asr.active_api_profiles.alibaba_cloud;
    const next = recognitionProfiles.find((profile) => profile.id === activeId)
      ?? recognitionProfiles[0];
    setSelectedProfileId(next?.id ?? "");
    setTestedProfileId("");
  }, [provider, recognitionProfiles, selectedProfileId, settings.asr.active_api_profiles]);

  const setCloudRecognitionBackend = (backend: CloudBackend) => {
    setCloudBackend(backend);
    setSelectedProfileId("");
    setTestedProfileId("");
    setApiEditor(null);
  };

  const selectProfile = (profileId: string) => {
    setSelectedProfileId(profileId);
    setTestedProfileId("");
  };

  const addApiProfile = () => setApiEditor(createApiProfileDraft(provider));

  const saveApiEditor = async () => {
    if (!apiEditor) return;
    clearMessage();
    const saved = await apiProfiles.create(
      apiProfileFromEditorDraft(apiEditor),
      apiEditor.api_key,
    );
    if (!saved) return;
    setSelectedProfileId(saved.id);
    setTestedProfileId("");
    setApiEditor(null);
  };

  const testAndApplyCloud = async () => {
    if (!selectedProfile) return;
    setBusy(true);
    clearMessage();
    try {
      const tested = await apiProfiles.test(selectedProfile.id, "asr", cloudBackend);
      if (!tested) return;
      const latest = await coreApi.settings();
      const selectedAsr = selectRecognitionSource(latest.asr, selectedProfile.id);
      await onSave({
        ...latest,
        asr: { ...selectedAsr, backend: cloudBackend },
      });
      setTestedProfileId(selectedProfile.id);
      showInfo(t("onboarding.recognition.connectionReady"));
    } catch (reason) {
      showError(reason, "errors.apiProfiles.operation");
    } finally {
      setBusy(false);
    }
  };

  const finishRecognition = async () => {
    if (recognitionMode === "cloud") {
      if (!selectedProfile || testedProfileId !== selectedProfile.id) return;
      await onFinish();
      return;
    }
    if (!localReady || saveBusy) return;
    setBusy(true);
    try {
      const latest = await coreApi.settings();
      const draft = draftController.getCurrent();
      await onSave({
        ...latest,
        asr: {
          ...latest.asr,
          backend: "local_whisper",
          language: draft.asr.language,
          local: draft.asr.local,
        },
      });
      await onFinish();
    } catch (reason) {
      showError(reason, "errors.settings.apply");
    } finally {
      setBusy(false);
    }
  };

  return {
    recognitionMode,
    setRecognitionMode,
    cloudBackend,
    setCloudBackend: setCloudRecognitionBackend,
    selectedProfileId,
    testedProfileId,
    selectProfile,
    selectedProfile,
    apiEditor,
    setApiEditor,
    addApiProfile,
    saveApiEditor,
    cancelApiEditor: () => setApiEditor(null),
    testAndApplyCloud,
    finishRecognition,
    provider,
    recognitionProfiles,
    apiProfiles,
    draftController,
    asr,
    localSettingsError,
    localReady,
    saveBusy,
    busy,
  };
}
