import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-dialog";

import { providersApi } from "../../providers/api";
import { localizedError } from "../../app/app-utils";
import type {
  ApiProfileView,
  QwenModelRecord,
  QwenRuntimeStatus,
  ProviderDefinition,
} from "../../providers/types";
import type { Settings } from "../types";
import {
  recognitionServicesForProfile,
  selectRecognitionService,
} from "../../recognition-services";
import { selectRecognitionSource } from "../settings-derived";
import type { SettingsDraftController } from "./useSettingsDraft";

export function useAsrModels({
  active,
  settings,
  onModelsChanged,
  draftController,
  apiProfiles,
  providerDefinitions,
}: {
  active: boolean;
  settings: Settings;
  onModelsChanged: () => Promise<void>;
  draftController: SettingsDraftController;
  apiProfiles: ApiProfileView[];
  providerDefinitions: ProviderDefinition[];
}) {
  const { t } = useTranslation();
  const [qwenModels, setQwenModels] = useState<QwenModelRecord[]>([]);
  const qwenModelsRef = useRef(qwenModels);
  qwenModelsRef.current = qwenModels;
  const [qwenModelsReady, setQwenModelsReady] = useState(false);
  const [qwenRuntime, setQwenRuntime] = useState<QwenRuntimeStatus | null>(null);
  const [qwenMessage, setQwenMessage] = useState("");
  const [modelDirectoryText, setModelDirectoryText] = useState(settings.storage.model_directory);

  useEffect(() => {
    setModelDirectoryText(settings.storage.model_directory);
  }, [settings.storage.model_directory]);

  const fetchQwenModels = useCallback(async (isCancelled: () => boolean) => {
    try {
      const [next, runtime] = await Promise.all([providersApi.qwenModels(), providersApi.qwenRuntime()]);
      if (isCancelled()) return;
      const wasBusy = qwenModelsRef.current.some((model) => ["downloading", "verifying"].includes(model.status));
      qwenModelsRef.current = next;
      setQwenModels(next);
      if (wasBusy && !next.some((model) => ["downloading", "verifying"].includes(model.status))) {
        void onModelsChanged();
      }
      setQwenRuntime(runtime);
      setQwenModelsReady(true);
    } catch (reason) {
      if (isCancelled()) return;
      setQwenModelsReady(false);
      setQwenMessage(localizedError(reason, t, "errors.asr.models"));
    }
  }, [onModelsChanged, t]);
  const loadQwenModels = useCallback(
    () => fetchQwenModels(() => false),
    [fetchQwenModels],
  );

  useEffect(() => {
    let cancelled = false;
    let timer: number | null = null;
    const poll = async () => {
      await fetchQwenModels(() => cancelled);
      if (!cancelled && active) timer = window.setTimeout(() => void poll(), 750);
    };
    void poll();
    return () => {
      cancelled = true;
      if (timer !== null) window.clearTimeout(timer);
    };
  }, [active, fetchQwenModels]);

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

  const updateManagedQwen = <K extends keyof Settings["asr"]["managed_qwen"]>(
    key: K,
    value: Settings["asr"]["managed_qwen"][K],
  ) => {
    draftController.applySettings((current) => ({
      ...current,
      asr: { ...current.asr, managed_qwen: { ...current.asr.managed_qwen, [key]: value } },
    }));
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
        void loadQwenModels();
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

  const runQwenAction = async (
    model: QwenModelRecord,
    action: (id: string) => Promise<unknown>,
  ) => {
    try {
      await action(model.id);
      setQwenMessage("");
      await loadQwenModels();
      await onModelsChanged();
    } catch (reason) {
      setQwenMessage(localizedError(reason, t, "errors.asr.models"));
    }
  };
  const removeQwenModel = async (model: QwenModelRecord) => {
    await runQwenAction(model, providersApi.deleteQwenModel);
  };

  const runQwenRuntimeAction = async (action: () => Promise<unknown>) => {
    try {
      await action();
      setQwenMessage("");
      await loadQwenModels();
    } catch (reason) {
      setQwenMessage(localizedError(reason, t, "errors.asr.download"));
    }
  };

  return {
    qwenModels,
    qwenModelsReady,
    qwenRuntime,
    qwenMessage,
    modelDirectoryText,
    setModelDirectoryText,
    loadQwenModels,
    updateAsr,
    updateRecognitionSource,
    updateRecognitionService,
    updateManagedQwen,
    updateVad,
    updateModelDirectory,
    chooseModelDirectory,
    downloadQwenModel: (model: QwenModelRecord) => runQwenAction(model, providersApi.downloadQwenModel),
    downloadQwenRuntime: () => runQwenRuntimeAction(providersApi.downloadQwenRuntime),
    cancelQwenRuntimeDownload: () => runQwenRuntimeAction(providersApi.cancelQwenRuntimeDownload),
    cancelQwenDownload: (model: QwenModelRecord) => runQwenAction(model, providersApi.cancelQwenDownload),
    verifyQwenModel: (model: QwenModelRecord) => runQwenAction(model, providersApi.verifyQwenModel),
    removeQwenModel,
  };
}
