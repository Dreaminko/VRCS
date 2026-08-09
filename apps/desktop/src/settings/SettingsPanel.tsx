import { useState } from "react";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import {
  AudioLines,
  BookOpen,
  KeyRound,
  Languages,
  PlusCircle,
  SlidersHorizontal,
  Volume2,
  Wrench,
} from "lucide-react";

import {
  asrSelectionError,
  audioSelectionErrors,
  validComputeTypes,
} from "../settings-validation";
import type {
  AsrCapabilities,
  AudioDevice,
  DictionarySource,
  Settings,
} from "../types";
import { useAnkiSettings } from "./hooks/useAnkiSettings";
import { useAsrModels } from "./hooks/useAsrModels";
import { useDesktopPreferences } from "./hooks/useDesktopPreferences";
import { useDictionaryActions } from "./hooks/useDictionaryActions";
import { useSettingsDraft } from "./hooks/useSettingsDraft";
import { createDebugRows } from "./settings-derived";
import { AnkiSettingsSection } from "./sections/AnkiSettingsSection";
import { ApiManagementSettingsSection } from "./sections/ApiManagementSettingsSection";
import { AudioSettingsSection } from "./sections/AudioSettingsSection";
import { DebugSettingsSection } from "./sections/DebugSettingsSection";
import { DictionarySettingsSection } from "./sections/DictionarySettingsSection";
import { RecognitionSettingsSection } from "./sections/RecognitionSettingsSection";
import { SystemSettingsSection } from "./sections/SystemSettingsSection";
import { TranslationSettingsSection } from "./sections/TranslationSettingsSection";
import type { SettingsCategory } from "./settings-types";

export function SettingsPanel({
  settings,
  interfaceScale,
  devices,
  devicesReady,
  dictionaries,
  disabled,
  modelStatus,
  asrCapabilities,
  onRefresh,
  onImportDictionary,
  onDeleteDictionary,
  onModelsChanged,
  onInterfaceScaleChange,
  onSave,
}: {
  settings: Settings;
  interfaceScale: number;
  devices: AudioDevice[];
  devicesReady: boolean;
  dictionaries: DictionarySource[];
  disabled: boolean;
  modelStatus: string;
  asrCapabilities: AsrCapabilities | null;
  onRefresh: () => Promise<void>;
  onImportDictionary: (
    file: File,
    onProgress?: (progress: number) => void,
  ) => Promise<DictionarySource>;
  onDeleteDictionary: (id: number) => Promise<void>;
  onModelsChanged: () => Promise<void>;
  onInterfaceScaleChange: (value: number) => void;
  onSave: (value: Settings) => Promise<Settings>;
}) {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage ?? "en-US";
  const [activeCategory, setActiveCategory] = useState<SettingsCategory>("system");

  const draftController = useSettingsDraft(settings, onSave);
  const desktop = useDesktopPreferences();
  const asr = useAsrModels({
    active: activeCategory === "recognition",
    settings,
    modelStatus,
    asrCapabilities,
    onModelsChanged,
    draftController,
  });
  const anki = useAnkiSettings({
    active: activeCategory === "anki",
    settings,
    draftController,
  });
  const dictionary = useDictionaryActions({
    locale,
    onImport: onImportDictionary,
    onDelete: onDeleteDictionary,
  });

  const { draft, saveState, applySettings } = draftController;
  const desktopPreferences = desktop.desktopPreferences;
  const desktopPreferencesReady = desktop.ready;
  const desktopSaveState = desktop.saveState;
  const uiLanguagePreference = desktop.uiLanguagePreference;
  const updateDesktop = desktop.updateDesktop;
  const updateUiLanguage = desktop.updateUiLanguage;

  const managedModels = asr.managedModels;
  const modelsReady = asr.modelsReady;
  const modelMessage = asr.message;
  const modelDirectoryText = asr.modelDirectoryText;
  const modelStatusLabel = asr.modelStatusLabel;
  const installedModels = asr.installed;
  const downloadingModels = asr.downloading;
  const selectableModels = asr.selectable;
  const loadModels = asr.loadModels;
  const updateAsr = asr.updateAsr;
  const updateRecognitionSource = asr.updateRecognitionSource;
  const updateLocalAsr = asr.updateLocalAsr;
  const updateVad = asr.updateVad;
  const setModelDirectoryText = asr.setModelDirectoryText;
  const updateModelDirectory = asr.updateModelDirectory;
  const chooseModelDirectory = asr.chooseModelDirectory;
  const downloadModel = asr.downloadModel;
  const removeModel = asr.removeModel;

  const ankiStatus = anki.status;
  const ankiBusy = anki.busy;
  const ankiMessage = anki.message;
  const ankiPortText = anki.portText;
  const ankiPortError = anki.portError;
  const ankiDeckNames = anki.decks;
  const ankiModelOptions = anki.models;
  const ankiFieldOptions = anki.frontFields;
  const ankiBackFieldOptions = anki.backFields;
  const loadAnkiStatus = anki.loadStatus;
  const setAnkiPortText = anki.setPortText;
  const commitAnkiPort = anki.commitPort;
  const updateAnki = anki.update;

  const dictionaryBusy = dictionary.busy;
  const dictionaryMessage = dictionary.message;
  const dictionaryProgress = dictionary.progress;
  const dictionaryFileRef = dictionary.fileInputRef;
  const chooseDictionary = dictionary.choose;
  const removeDictionary = dictionary.remove;

  const outputDevices = devices.filter((device) => device.is_loopback);
  const microphoneDevices = devices.filter((device) => !device.is_loopback);
  const deviceErrors = devicesReady
    ? audioSelectionErrors(draft, devices, (key) => t(key))
    : [];
  const asrError = asrSelectionError(draft, asrCapabilities, (key) => t(key));
  const computeTypes = validComputeTypes(asrCapabilities, draft.asr.local.device);

  const settingsCategories: Array<{
    id: SettingsCategory;
    label: string;
    icon: ReactNode;
  }> = [
    { id: "system", label: t("settings.categories.system"), icon: <SlidersHorizontal size={18} /> },
    { id: "audio", label: t("settings.categories.audio"), icon: <Volume2 size={18} /> },
    { id: "recognition", label: t("settings.categories.recognition"), icon: <AudioLines size={18} /> },
    { id: "translation", label: t("settings.categories.translation"), icon: <Languages size={18} /> },
    { id: "api", label: t("settings.categories.api"), icon: <KeyRound size={18} /> },
    { id: "dictionary", label: t("settings.categories.dictionary"), icon: <BookOpen size={18} /> },
    { id: "anki", label: "Anki", icon: <PlusCircle size={18} /> },
    { id: "debug", label: "Debug", icon: <Wrench size={18} /> },
  ];

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
      <div className="settings-tabbar-wrap">
        <div className="settings-tabbar" role="tablist" aria-label={t("settings.categories.label")}>
          {settingsCategories.map((category) => {
            const active = activeCategory === category.id;
            return (
              <button
                key={category.id}
                id={`settings-tab-${category.id}`}
                className={active ? "active" : ""}
                type="button"
                role="tab"
                aria-selected={active}
                aria-controls={`settings-panel-${category.id}`}
                aria-label={category.label}
                onClick={() => setActiveCategory(category.id)}
              >
                <span className="settings-tab-icon" aria-hidden="true">{category.icon}</span>
                <span className="settings-tab-label">{category.label}</span>
              </button>
            );
          })}
        </div>
      </div>

      {activeCategory === "system" && (
        <SystemSettingsSection
          desktopPreferences={desktopPreferences}
          desktopPreferencesReady={desktopPreferencesReady}
          desktopSaveState={desktopSaveState}
          uiLanguagePreference={uiLanguagePreference}
          interfaceScale={interfaceScale}
          onUpdateDesktop={updateDesktop}
          onInterfaceScaleChange={onInterfaceScaleChange}
          onUpdateUiLanguage={updateUiLanguage}
        />
      )}

      {activeCategory === "recognition" && (
        <RecognitionSettingsSection
          locale={locale}
          draft={draft}
          disabled={disabled}
          modelStatus={modelStatus}
          status={{
            capabilities: asrCapabilities,
            error: asrError,
            modelStatusLabel,
            computeTypes,
            selectableModels,
          }}
          models={{
            installed: installedModels,
            downloading: downloadingModels,
            managed: managedModels,
            ready: modelsReady,
            message: modelMessage,
            directoryText: modelDirectoryText,
          }}
          saveState={saveState}
          actions={{
            updateAsr,
            updateRecognitionSource,
            updateLocalAsr,
            updateVad,
            loadModels,
            setModelDirectoryText,
            updateModelDirectory,
            chooseModelDirectory,
            downloadModel,
            removeModel,
          }}
        />
      )}

      {activeCategory === "audio" && (
        <AudioSettingsSection
          draft={draft}
          devices={devices}
          devicesReady={devicesReady}
          deviceErrors={deviceErrors}
          outputDevices={outputDevices}
          microphoneDevices={microphoneDevices}
          saveState={saveState}
          onRefresh={onRefresh}
          applySettings={applySettings}
        />
      )}

      {activeCategory === "translation" && (
        <TranslationSettingsSection
          draft={draft}
          disabled={disabled}
          saveState={saveState}
          applySettings={applySettings}
        />
      )}

      {activeCategory === "api" && (
        <ApiManagementSettingsSection
          disabled={disabled}
          onRefresh={onRefresh}
        />
      )}

      {activeCategory === "dictionary" && (
        <DictionarySettingsSection
          locale={locale}
          selectionLookupEnabled={draft.dictionary.selection_lookup_enabled}
          dictionaries={dictionaries}
          busy={dictionaryBusy}
          message={dictionaryMessage}
          progress={dictionaryProgress}
          fileInputRef={dictionaryFileRef}
          saveState={saveState}
          onSelectionLookupChange={(enabled) => applySettings((current) => ({
            ...current,
            dictionary: {
              ...current.dictionary,
              selection_lookup_enabled: enabled,
            },
          }))}
          onChoose={chooseDictionary}
          onRemove={removeDictionary}
        />
      )}

      {activeCategory === "anki" && (
        <AnkiSettingsSection
          draft={draft}
          status={ankiStatus}
          busy={ankiBusy}
          message={ankiMessage}
          portText={ankiPortText}
          portError={ankiPortError}
          saveState={saveState}
          deckNames={ankiDeckNames}
          modelOptions={ankiModelOptions}
          frontFieldOptions={ankiFieldOptions}
          backFieldOptions={ankiBackFieldOptions}
          onLoadStatus={loadAnkiStatus}
          onSetPortText={setAnkiPortText}
          onCommitPort={commitAnkiPort}
          onUpdate={updateAnki}
        />
      )}

      {activeCategory === "debug" && (
        <DebugSettingsSection rows={debugRows} />
      )}
    </section>
  );
}
