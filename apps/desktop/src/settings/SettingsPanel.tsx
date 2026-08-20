import { useState } from "react";
import { useTranslation } from "react-i18next";

import {
  asrSelectionError,
  audioSelectionErrors,
  validComputeTypes,
} from "./settings-validation";
import type {
  AsrCapabilities,
  AudioDevice,
  DictionarySource,
  Health,
  Settings,
} from "../types";
import { SettingsTabBar } from "./components/SettingsTabBar";
import { useAnkiSettings } from "./hooks/useAnkiSettings";
import { useApiProfileViews } from "./hooks/useApiProfileViews";
import { useAsrModels } from "./hooks/useAsrModels";
import { useDesktopPreferences } from "./hooks/useDesktopPreferences";
import { useDictionaryActions } from "./hooks/useDictionaryActions";
import { useSettingsDraft } from "./hooks/useSettingsDraft";
import { createDebugRows } from "./settings-derived";
import { ApiManagementSettingsSection } from "./sections/ApiManagementSettingsSection";
import { AudioSettingsSection } from "./sections/AudioSettingsSection";
import { DebugSettingsSection } from "./sections/DebugSettingsSection";
import { GlossarySettingsSection } from "./sections/GlossarySettingsSection";
import { LearningSettingsSection } from "./sections/LearningSettingsSection";
import { RecognitionSettingsSection } from "./sections/RecognitionSettingsSection";
import { ConnectionSettingsSection } from "./sections/ConnectionSettingsSection";
import { SystemSettingsSection } from "./sections/SystemSettingsSection";
import { TranslationSettingsSection } from "./sections/TranslationSettingsSection";
import { VrOverlaySettingsSection } from "./sections/VrOverlaySettingsSection";
import type { SettingsCategory } from "./settings-types";
import type { AppUpdaterState } from "../updates/useAppUpdater";

export function SettingsPanel({
  initialCategory = "system",
  settings,
  health,
  interfaceScale,
  devices,
  devicesReady,
  onStartMicrophoneTest,
  onStopMicrophoneTest,
  dictionaries,
  disabled,
  modelStatus,
  asrCapabilities,
  onRefresh,
  onRefreshSettings,
  onImportDictionary,
  onDeleteDictionary,
  onModelsChanged,
  onInterfaceScaleChange,
  onSave,
  onTestOsc,
  onStartOnboarding,
  updater,
}: {
  initialCategory?: SettingsCategory;
  settings: Settings;
  health: Health | null;
  interfaceScale: number;
  devices: AudioDevice[];
  devicesReady: boolean;
  onStartMicrophoneTest: () => Promise<void>;
  onStopMicrophoneTest: () => Promise<void>;
  dictionaries: DictionarySource[];
  disabled: boolean;
  modelStatus: string;
  asrCapabilities: AsrCapabilities | null;
  onRefresh: () => Promise<void>;
  onRefreshSettings: () => Promise<void>;
  onImportDictionary: (
    file: File,
    onProgress?: (progress: number) => void,
  ) => Promise<DictionarySource>;
  onDeleteDictionary: (id: number) => Promise<void>;
  onModelsChanged: () => Promise<void>;
  onInterfaceScaleChange: (value: number) => void;
  onSave: (value: Settings) => Promise<Settings>;
  onTestOsc: () => Promise<void>;
  onStartOnboarding: () => void;
  updater: AppUpdaterState;
}) {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage ?? "en-US";
  const [activeCategory, setActiveCategory] = useState<SettingsCategory>(initialCategory);
  const apiProfileCatalog = useApiProfileViews(`${settings.asr.active_profile_id ?? "local"}:${settings.asr.backend}`);
  const draftController = useSettingsDraft(settings, onSave);
  const desktop = useDesktopPreferences();
  const asr = useAsrModels({
    active: activeCategory === "recognition",
    settings,
    modelStatus,
    asrCapabilities,
    onModelsChanged,
    draftController,
    apiProfiles: apiProfileCatalog.profiles,
    providerDefinitions: apiProfileCatalog.providerDefinitions,
  });
  const anki = useAnkiSettings({
    active: activeCategory === "connections",
    settings,
    draftController,
  });
  const dictionary = useDictionaryActions({
    locale,
    onImport: onImportDictionary,
    onDelete: onDeleteDictionary,
  });

  const { draft, saveState, applySettings } = draftController;

  const outputDevices = devices.filter((device) => device.is_loopback);
  const microphoneDevices = devices.filter((device) => !device.is_loopback);
  const deviceErrors = devicesReady
    ? audioSelectionErrors(draft, devices, (key) => t(key))
    : [];
  const asrError = asrSelectionError(draft, asrCapabilities, (key) => t(key));
  const computeTypes = validComputeTypes(asrCapabilities, draft.asr.local.device);


  const debugRows = createDebugRows({
    draft,
    modelStatus,
    asrCapabilities,
    disabled,
    outputDeviceCount: outputDevices.length,
    microphoneDeviceCount: microphoneDevices.length,
    dictionaryCount: dictionaries.length,
    locale,
    t,
  });

  return (
    <section className="settings-surface">
      <SettingsTabBar activeCategory={activeCategory} onChange={setActiveCategory} />

      {activeCategory === "system" && (
        <SystemSettingsSection
          desktopPreferences={desktop.desktopPreferences}
          desktopPreferencesReady={desktop.ready}
          desktopSaveState={desktop.saveState}
          uiLanguagePreference={desktop.uiLanguagePreference}
          interfaceScale={interfaceScale}
          onUpdateDesktop={desktop.updateDesktop}
          onInterfaceScaleChange={onInterfaceScaleChange}
          onUpdateUiLanguage={desktop.updateUiLanguage}
          onboardingDisabled={health?.capture_requested ?? false}
          onStartOnboarding={onStartOnboarding}
          locale={locale}
          draft={draft}
          saveState={saveState}
          applySettings={applySettings}
          updater={updater}
        />
      )}

      {activeCategory === "recognition" && (
        <RecognitionSettingsSection
          locale={locale}
          draft={draft}
          apiProfiles={apiProfileCatalog.profiles}
          providerDefinitions={apiProfileCatalog.providerDefinitions}
          modelStatus={modelStatus}
          status={{
            capabilities: asrCapabilities,
            error: asrError,
            modelStatusLabel: asr.modelStatusLabel,
            computeTypes,
            selectableModels: asr.selectable,
          }}
          models={{
            installed: asr.installed,
            downloading: asr.downloading,
            managed: asr.managedModels,
            ready: asr.modelsReady,
            message: asr.message,
            directoryText: asr.modelDirectoryText,
          }}
          saveState={saveState}
          actions={{
            updateAsr: asr.updateAsr,
            updateRecognitionSource: asr.updateRecognitionSource,
            updateRecognitionService: asr.updateRecognitionService,
            updateLocalAsr: asr.updateLocalAsr,
            updateVad: asr.updateVad,
            loadModels: asr.loadModels,
            setModelDirectoryText: asr.setModelDirectoryText,
            updateModelDirectory: asr.updateModelDirectory,
            chooseModelDirectory: asr.chooseModelDirectory,
            downloadModel: asr.downloadModel,
            removeModel: asr.removeModel,
          }}
        />
      )}

      {activeCategory === "audio" && (
        <AudioSettingsSection
          draft={draft}
          devices={devices}
          devicesReady={devicesReady}
          microphoneRunning={Boolean(health?.capture_running && health.microphone_device)}
          transcriptionRunning={health?.capture_requested ?? false}
          microphoneTestRunning={health?.microphone_test_running ?? false}
          deviceErrors={deviceErrors}
          outputDevices={outputDevices}
          microphoneDevices={microphoneDevices}
          saveState={saveState}
          onRefresh={onRefresh}
          applySettings={applySettings}
          onStartMicrophoneTest={onStartMicrophoneTest}
          onStopMicrophoneTest={onStopMicrophoneTest}
        />
      )}

      {activeCategory === "translation" && (
        <TranslationSettingsSection
          draft={draft}
          apiProfiles={apiProfileCatalog.profiles}
          saveState={saveState}
          applySettings={applySettings}
        />
      )}

      {activeCategory === "glossary" && (
        <GlossarySettingsSection
          draft={draft}
          saveState={saveState}
          applySettings={applySettings}
        />
      )}

      {activeCategory === "api" && (
        <ApiManagementSettingsSection
          settings={draft}
          onRefreshSettings={onRefreshSettings}
        />
      )}

      {activeCategory === "learning" && (
        <LearningSettingsSection
          locale={locale}
          selectionLookupEnabled={draft.dictionary.selection_lookup_enabled}
          dictionaries={dictionaries}
          dictionaryBusy={dictionary.busy}
          dictionaryMessage={dictionary.message}
          dictionaryProgress={dictionary.progress}
          dictionaryFileRef={dictionary.fileInputRef}
          saveState={saveState}
          onSelectionLookupChange={(enabled) => applySettings((current) => ({
            ...current,
            dictionary: {
              ...current.dictionary,
              selection_lookup_enabled: enabled,
            },
          }))}
          onChooseDictionary={dictionary.choose}
          onRemoveDictionary={dictionary.remove}
        />
      )}

      {activeCategory === "connections" && (
        <ConnectionSettingsSection
          draft={draft}
          health={health}
          saveState={saveState}
          applySettings={applySettings}
          onTest={onTestOsc}
          anki={{
            status: anki.status,
            busy: anki.busy,
            message: anki.message,
            portText: anki.portText,
            portError: anki.portError,
            deckNames: anki.decks,
            modelOptions: anki.models,
            frontFieldOptions: anki.frontFields,
            backFieldOptions: anki.backFields,
            onLoadStatus: anki.loadStatus,
            onSetPortText: anki.setPortText,
            onCommitPort: anki.commitPort,
            onUpdate: anki.update,
          }}
        />
      )}

      {activeCategory === "vr_overlay" && (
        <VrOverlaySettingsSection
          draft={draft}
          saveState={saveState}
          applySettings={applySettings}
        />
      )}

      {activeCategory === "debug" && (
        <DebugSettingsSection rows={debugRows} />
      )}
    </section>
  );
}
