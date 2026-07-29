import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-dialog";

import { coreApi } from "../../api";
import { localizedError } from "../../app-utils";
import { validComputeTypes } from "../../settings-validation";
import type { AsrCapabilities, AsrModelRecord, Settings } from "../../types";
import {
  classifyModels,
  MODEL_PRESENTATION,
  modelStatusLabel,
} from "../settings-derived";
import type { SettingsDraftController } from "./useSettingsDraft";

export function useAsrModels({
  active,
  settings,
  modelStatus,
  asrCapabilities,
  onModelsChanged,
  draftController,
}: {
  active: boolean;
  settings: Settings;
  modelStatus: string;
  asrCapabilities: AsrCapabilities | null;
  onModelsChanged: () => Promise<void>;
  draftController: SettingsDraftController;
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

  const loadModels = useCallback(async () => {
    try {
      const previous = managedModelsRef.current;
      const next = await coreApi.asrModels();
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
      setModelsReady(false);
      setMessage(localizedError(reason, t, "errors.asr.models"));
    }
  }, [onModelsChanged]);

  useEffect(() => {
    void loadModels();
  }, [loadModels]);

  useEffect(() => {
    if (!active) return;
    const timer = window.setInterval(() => void loadModels(), 750);
    return () => window.clearInterval(timer);
  }, [active, loadModels]);

  const updateAsr = <K extends keyof Settings["asr"]>(
    key: K,
    value: Settings["asr"][K],
  ) => {
    draftController.applySettings((current) => {
      const nextAsr = { ...current.asr, [key]: value };
      if (key === "device") {
        const allowed = validComputeTypes(asrCapabilities, nextAsr.device);
        if (!allowed.includes(nextAsr.compute_type)) {
          nextAsr.compute_type = allowed[0] ?? "int8";
        }
      }
      return { ...current, asr: nextAsr };
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
      await coreApi.downloadAsrModel(model.id);
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
      await coreApi.deleteAsrModel(model.id);
      await loadModels();
      await onModelsChanged();
      setMessage(t("settings.recognition.deleted", { name }));
    } catch (reason) {
      setMessage(localizedError(reason, t, "errors.asr.delete"));
    }
  };

  const selectedModelCapability = asrCapabilities?.models.find(
    (model) => model.id === draftController.draft.asr.model,
  );
  const selectedModelStatus = (
    draftController.draft.asr.model === settings.asr.model
    && ["loading", "ready", "error"].includes(modelStatus)
  )
    ? modelStatus
    : selectedModelCapability?.status;
  const classified = classifyModels(
    managedModels,
    asrCapabilities,
    draftController.draft.asr.model,
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
    updateVad,
    updateModelDirectory,
    chooseModelDirectory,
    downloadModel,
    removeModel,
    modelStatusLabel: modelStatusLabel(selectedModelStatus, t),
    ...classified,
  };
}
