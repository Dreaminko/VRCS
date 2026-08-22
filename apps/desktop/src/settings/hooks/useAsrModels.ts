import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-dialog";

import { providersApi } from "../../providers/api";
import { localizedError } from "../../app/app-utils";
import { validComputeTypes } from "../settings-validation";
import type {
  ApiProfileView,
  AsrCapabilities,
  AsrModelRecord,
  ProviderDefinition,
} from "../../providers/types";
import type { Settings } from "../types";
import {
  recognitionServicesForProfile,
  selectRecognitionService,
} from "../../recognition-services";
import {
  classifyModels,
  MODEL_PRESENTATION,
  modelStatusLabel,
  selectRecognitionSource,
} from "../settings-derived";
import type { SettingsDraftController } from "./useSettingsDraft";

export function useAsrModels({
  active,
  settings,
  modelStatus,
  asrCapabilities,
  onModelsChanged,
  draftController,
  apiProfiles,
  providerDefinitions,
}: {
  active: boolean;
  settings: Settings;
  modelStatus: string;
  asrCapabilities: AsrCapabilities | null;
  onModelsChanged: () => Promise<void>;
  draftController: SettingsDraftController;
  apiProfiles: ApiProfileView[];
  providerDefinitions: ProviderDefinition[];
}) {
  const { t } = useTranslation();
  const [managedModels, setManagedModels] = useState<AsrModelRecord[]>([]);
  const [modelsReady, setModelsReady] = useState(false);
  const [message, setMessage] = useState("");
  const [modelDirectoryText, setModelDirectoryText] = useState(settings.storage.model_directory);
  const managedModelsRef = useRef(managedModels);
  managedModelsRef.current = managedModels;

  useEffect(() => {
    setModelDirectoryText(settings.storage.model_directory);
  }, [settings.storage.model_directory]);

  const fetchModels = useCallback(async (isCancelled: () => boolean) => {
    try {
      const previous = managedModelsRef.current;
      const next = await providersApi.asrModels();
      if (isCancelled()) return;
      managedModelsRef.current = next;
      setManagedModels(next);
      setModelsReady(true);
      if (
        previous.some((model) => model.status === "downloading")
        && !next.some((model) => model.status === "downloading")
      ) {
        void onModelsChanged();
      }
    } catch (reason) {
      if (isCancelled()) return;
      setModelsReady(false);
      setMessage(localizedError(reason, t, "errors.asr.models"));
    }
  }, [onModelsChanged]);

  const loadModels = useCallback(
    () => fetchModels(() => false),
    [fetchModels],
  );

  useEffect(() => {
    let cancelled = false;
    let timer: number | null = null;
    const poll = async () => {
      await fetchModels(() => cancelled);
      if (!cancelled && active) timer = window.setTimeout(() => void poll(), 750);
    };
    void poll();
    return () => {
      cancelled = true;
      if (timer !== null) window.clearTimeout(timer);
    };
  }, [active, fetchModels]);

  const updateAsr = <K extends keyof Settings["asr"]>(
    key: K,
    value: Settings["asr"][K],
  ) => {
    draftController.applySettings((current) => {
      const nextAsr = { ...current.asr, [key]: value };
      return { ...current, asr: nextAsr };
    });
  };

  const updateRecognitionSource = (source: string) => {
    draftController.applySettings((current) => ({
      ...current,
      asr: selectRecognitionSource(current.asr, source, apiProfiles, providerDefinitions),
    }));
  };

  const updateRecognitionService = (serviceId: string) => {
    draftController.applySettings((current) => {
      const profile = apiProfiles.find((item) => item.id === current.asr.active_profile_id);
      const service = recognitionServicesForProfile(profile, providerDefinitions)
        .find((item) => item.id === serviceId);
      return service
        ? { ...current, asr: selectRecognitionService(current.asr, service) }
        : current;
    });
  };

  const updateLocalAsr = <K extends keyof Settings["asr"]["local"]>(
    key: K,
    value: Settings["asr"]["local"][K],
  ) => {
    draftController.applySettings((current) => {
      const local = { ...current.asr.local, [key]: value };
      if (key === "device") {
        const allowed = validComputeTypes(asrCapabilities, local.device);
        if (!allowed.includes(local.compute_type)) local.compute_type = allowed[0] ?? "int8";
      }
      return { ...current, asr: { ...current.asr, local } };
    });
  };

  const updateVad = <K extends keyof Settings["vad"]>(
    key: K,
    value: Settings["vad"][K],
  ) => {
    draftController.applySettings((current) => ({
      ...current,
      vad: { ...current.vad, [key]: value },
    }));
  };

  const updateModelDirectory = (value: string) => {
    const directory = value.trim();
    if (!directory) {
      draftController.setFailure(t("settings.recognition.modelDirectoryRequired"));
      return;
    }
    setModelDirectoryText(directory);
    if (directory === draftController.getCurrent().storage.model_directory) return;
    draftController.applySettings(
      (current) => ({
        ...current,
        storage: { ...current.storage, model_directory: directory },
      }),
      () => {
        void loadModels();
        void onModelsChanged();
      },
    );
  };

  const chooseModelDirectory = async () => {
    try {
      const directory = await open({
        directory: true,
        multiple: false,
        title: t("settings.recognition.chooseModelDirectory"),
      });
      if (typeof directory === "string") updateModelDirectory(directory);
    } catch (reason) {
      draftController.setFailure(localizedError(reason, t, "errors.dialog.folder"));
    }
  };

  const downloadModel = async (model: AsrModelRecord) => {
    setMessage(t("settings.recognition.preparingDownload", {
      name: MODEL_PRESENTATION[model.id].name,
    }));
    try {
      await providersApi.downloadAsrModel(model.id);
      setMessage(t("settings.recognition.downloadQueued", {
        name: MODEL_PRESENTATION[model.id].name,
      }));
      await loadModels();
    } catch (reason) {
      setMessage(localizedError(reason, t, "errors.asr.download"));
    }
  };

  const removeModel = async (model: AsrModelRecord) => {
    const name = MODEL_PRESENTATION[model.id].name;
    if (!window.confirm(t("settings.recognition.confirmDelete", { name }))) return;
    setMessage(t("settings.recognition.deleting", { name }));
    try {
      await providersApi.deleteAsrModel(model.id);
      await loadModels();
      await onModelsChanged();
      setMessage(t("settings.recognition.deleted", { name }));
    } catch (reason) {
      setMessage(localizedError(reason, t, "errors.asr.delete"));
    }
  };

  const selectedModelCapability = asrCapabilities?.models.find(
    (model) => model.id === draftController.draft.asr.local.model,
  );
  const selectedModelStatus = (
    draftController.draft.asr.local.model === settings.asr.local.model
    && ["loading", "ready", "error"].includes(modelStatus)
  )
    ? modelStatus
    : selectedModelCapability?.status;
  const classified = classifyModels(
    managedModels,
    asrCapabilities,
    draftController.draft.asr.local.model,
    modelsReady,
  );

  return {
    managedModels,
    modelsReady,
    message,
    modelDirectoryText,
    setModelDirectoryText,
    loadModels,
    updateAsr,
    updateRecognitionSource,
    updateRecognitionService,
    updateLocalAsr,
    updateVad,
    updateModelDirectory,
    chooseModelDirectory,
    downloadModel,
    removeModel,
    modelStatusLabel: modelStatusLabel(selectedModelStatus, t),
    ...classified,
  };
}
