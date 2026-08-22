import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { settingsApi } from "../../settings/api";
import {
  apiProfileFromEditorDraft,
  createApiProfileDraft,
  type ApiProfileEditorDraft,
} from "../../api-profile-draft";
import { providerServicesWithCapability } from "../../provider-catalog";
import {
  recognitionProfiles as filterRecognitionProfiles,
  recognitionServicesForProfile,
  selectRecognitionProfile,
  selectRecognitionService,
} from "../../recognition-services";
import { useAsrModels } from "../../settings/hooks/useAsrModels";
import { useSettingsDraft } from "../../settings/hooks/useSettingsDraft";
import { asrSelectionError } from "../../settings/settings-validation";
import { useApiProfiles } from "../../settings/useApiProfiles";
import type { AsrCapabilities } from "../../providers/types";
import type { Settings } from "../../settings/types";
import type { RecognitionMode } from "../onboarding-types";

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
  const [selectedProfileId, setSelectedProfileId] = useState(
    settings.asr.active_profile_id ?? "",
  );
  const [selectedServiceId, setSelectedServiceId] = useState(
    settings.asr.backend === "local_whisper" ? "" : settings.asr.backend,
  );
  const [testedSelectionId, setTestedSelectionId] = useState("");
  const [apiEditor, setApiEditor] = useState<ApiProfileEditorDraft | null>(null);
  const [busy, setBusy] = useState(false);

  const draftController = useSettingsDraft(settings, onSave);
  const apiProfiles = useApiProfiles(onRefreshSettings);
  const recognitionProfiles = useMemo(
    () => filterRecognitionProfiles(apiProfiles.profiles),
    [apiProfiles.profiles],
  );
  const selectedProfile = recognitionProfiles.find((profile) => profile.id === selectedProfileId);
  const recognitionServices = useMemo(
    () => recognitionServicesForProfile(selectedProfile, apiProfiles.providerDefinitions),
    [apiProfiles.providerDefinitions, selectedProfile],
  );
  const selectedService = recognitionServices.find((service) => service.id === selectedServiceId);
  const selectionId = selectedProfile && selectedService
    ? `${selectedProfile.id}:${selectedService.id}`
    : "";
  const cloudReady = Boolean(selectionId && testedSelectionId === selectionId);

  const asr = useAsrModels({
    active: active && recognitionMode === "local",
    settings,
    modelStatus,
    asrCapabilities,
    onModelsChanged,
    draftController,
    apiProfiles: apiProfiles.profiles,
    providerDefinitions: apiProfiles.providerDefinitions,
  });
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
    if (selectedProfile) return;
    const next = recognitionProfiles.find((profile) => profile.id === settings.asr.active_profile_id)
      ?? recognitionProfiles[0];
    setSelectedProfileId(next?.id ?? "");
    setTestedSelectionId("");
  }, [recognitionProfiles, selectedProfile, settings.asr.active_profile_id]);

  useEffect(() => {
    if (selectedService) return;
    const next = recognitionServices.find((service) => service.id === settings.asr.backend)
      ?? recognitionServices[0];
    setSelectedServiceId(next?.id ?? "");
    setTestedSelectionId("");
  }, [recognitionServices, selectedService, settings.asr.backend]);

  const selectProfile = (profileId: string) => {
    setSelectedProfileId(profileId);
    setSelectedServiceId("");
    setTestedSelectionId("");
  };

  const selectService = (serviceId: string) => {
    setSelectedServiceId(serviceId);
    setTestedSelectionId("");
  };

  const addApiProfile = () => {
    const recognitionDefinitions = apiProfiles.providerDefinitions.filter(
      (definition) => providerServicesWithCapability(definition, "speech_to_text").length > 0,
    );
    setApiEditor(createApiProfileDraft(recognitionDefinitions));
  };

  const saveApiEditor = async () => {
    if (!apiEditor) return;
    clearMessage();
    const saved = await apiProfiles.create(
      apiProfileFromEditorDraft(apiEditor),
      apiEditor.api_key,
    );
    if (!saved) return;
    const service = recognitionServicesForProfile(saved, apiProfiles.providerDefinitions)[0];
    setSelectedProfileId(saved.id);
    setSelectedServiceId(service?.id ?? "");
    setTestedSelectionId("");
    setApiEditor(null);
  };

  const testAndApplyCloud = async () => {
    if (!selectedProfile || !selectedService) return;
    setBusy(true);
    clearMessage();
    try {
      const tested = await apiProfiles.test(
        selectedProfile.id,
        "speech_to_text",
        selectedService.id,
      );
      if (!tested?.ok) return;
      const activated = await apiProfiles.activate(selectedProfile.id, selectedService.id);
      if (!activated) return;
      const latest = await settingsApi.settings();
      const selectedAsr = selectRecognitionProfile(
        latest.asr,
        selectedProfile.id,
        apiProfiles.profiles,
        apiProfiles.providerDefinitions,
      );
      await onSave({
        ...latest,
        asr: selectRecognitionService(selectedAsr, selectedService),
      });
      setTestedSelectionId(`${selectedProfile.id}:${selectedService.id}`);
      showInfo(t("onboarding.recognition.connectionReady"));
    } catch (reason) {
      showError(reason, "errors.apiProfiles.operation");
    } finally {
      setBusy(false);
    }
  };

  const finishRecognition = async () => {
    if (recognitionMode === "cloud") {
      if (!cloudReady) return;
      await onFinish();
      return;
    }
    if (!localReady || saveBusy) return;
    setBusy(true);
    try {
      const latest = await settingsApi.settings();
      const draft = draftController.getCurrent();
      await onSave({
        ...latest,
        asr: {
          ...latest.asr,
          backend: "local_whisper",
          active_profile_id: null,
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
    selectedProfileId,
    selectedServiceId,
    testedSelectionId,
    cloudReady,
    selectProfile,
    selectService,
    selectedProfile,
    selectedService,
    recognitionServices,
    apiEditor,
    setApiEditor,
    addApiProfile,
    saveApiEditor,
    cancelApiEditor: () => setApiEditor(null),
    testAndApplyCloud,
    finishRecognition,
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
