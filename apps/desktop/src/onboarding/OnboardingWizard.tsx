import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  AudioLines,
  Check,
  ChevronLeft,
  ChevronRight,
  Cloud,
  HardDrive,
  KeyRound,
  Languages,
  Mic,
  RefreshCw,
  ShieldCheck,
  Sparkles,
  Volume2,
} from "lucide-react";

import { coreApi } from "../api";
import { supportsRecognition } from "../api-profile-purpose";
import { localizedError } from "../app-utils";
import { changeUiLanguage, currentUiLanguagePreference } from "../i18n";
import { useMicrophoneCalibration } from "../hooks/useMicrophoneCalibration";
import { localeCatalog } from "../i18n/catalog";
import { DEFAULT_MICROPHONE_THRESHOLD_DBFS } from "../microphone-calibration";
import type {
  ApiProvider,
  AsrCapabilities,
  AudioDevice,
  AudioLevel,
  Health,
  Settings,
} from "../types";
import type { UiLanguagePreference } from "../ui-language";
import {
  ApiProfileEditor,
  apiProfileFromEditorDraft,
  createApiProfileDraft,
  type ApiProfileEditorDraft,
} from "../settings/api/ApiProfileEditor";
import { useAsrModels } from "../settings/hooks/useAsrModels";
import { useSettingsDraft } from "../settings/hooks/useSettingsDraft";
import { LocalRecognitionSettings, LocalRuntimeStatus } from "../settings/recognition/LocalRecognitionSettings";
import { ModelManagerPanel } from "../settings/recognition/ModelManagerPanel";
import { selectRecognitionSource } from "../settings/settings-derived";
import { DeviceGroup, Select } from "../settings/SettingsControls";
import { MicrophoneLevelSetting } from "../settings/sections/MicrophoneLevelSetting";
import { useApiProfiles } from "../settings/useApiProfiles";
import {
  asrSelectionError,
  audioSelectionErrors,
  hasEnabledAudioSource,
  validComputeTypes,
} from "../settings-validation";

const STEP_COUNT = 5;

type RecognitionMode = "cloud" | "local";
type CloudBackend = "qwen_realtime" | "fun_asr_realtime" | "openai_realtime";

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

export function OnboardingWizard({
  initialStep,
  settings,
  health,
  devices,
  devicesReady,
  microphoneLevel,
  asrCapabilities,
  modelStatus,
  onRefreshDevices,
  onRefreshSettings,
  onModelsChanged,
  onStartMicrophoneTest,
  onStopMicrophoneTest,
  onSave,
  onProgress,
  onSkip,
  onComplete,
}: {
  initialStep: number;
  settings: Settings;
  health: Health | null;
  devices: AudioDevice[];
  devicesReady: boolean;
  microphoneLevel: AudioLevel | null;
  asrCapabilities: AsrCapabilities | null;
  modelStatus: string;
  onRefreshDevices: () => Promise<void>;
  onRefreshSettings: () => Promise<void>;
  onModelsChanged: () => Promise<void>;
  onStartMicrophoneTest: () => Promise<void>;
  onStopMicrophoneTest: () => Promise<void>;
  onSave: (settings: Settings) => Promise<Settings>;
  onProgress: (step: number) => Promise<void>;
  onSkip: () => Promise<void>;
  onComplete: (startCapture: boolean) => Promise<void>;
}) {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage ?? "en-US";
  const [step, setStep] = useState(Math.min(STEP_COUNT - 1, Math.max(0, initialStep)));
  const [message, setMessage] = useState("");
  const [messageTone, setMessageTone] = useState<"info" | "error">("error");
  const [busy, setBusy] = useState(false);
  const [languagePreference, setLanguagePreference] = useState<UiLanguagePreference>(
    currentUiLanguagePreference,
  );
  const [recognitionMode, setRecognitionMode] = useState<RecognitionMode>(
    settings.asr.backend === "local_whisper" ? "local" : "cloud",
  );
  const [cloudBackend, setCloudBackend] = useState<CloudBackend>(initialCloudBackend(settings));
  const [selectedProfileId, setSelectedProfileId] = useState("");
  const [testedProfileId, setTestedProfileId] = useState("");
  const [apiEditor, setApiEditor] = useState<ApiProfileEditorDraft | null>(null);
  const [audioBusy, setAudioBusy] = useState(false);
  const [microphoneTestBusy, setMicrophoneTestBusy] = useState(false);
  const [microphoneReviewed, setMicrophoneReviewed] = useState(
    settings.audio.microphone.mode === "disabled",
  );
  const microphoneStartedByWizardRef = useRef(false);
  const progressPendingRef = useRef(false);
  const calibration = useMicrophoneCalibration({
    level: microphoneLevel,
    testing: health?.microphone_test_running ?? false,
    onStartTest: async () => {
      await onStartMicrophoneTest();
      microphoneStartedByWizardRef.current = true;
    },
  });

  const draftController = useSettingsDraft(settings, onSave);
  const asr = useAsrModels({
    active: step === 1 && recognitionMode === "local",
    settings,
    modelStatus,
    asrCapabilities,
    onModelsChanged,
    draftController,
  });
  const apiProfiles = useApiProfiles(onRefreshSettings);
  const outputDevices = devices.filter((device) => device.is_loopback);
  const microphoneDevices = devices.filter((device) => !device.is_loopback);
  const provider = backendProvider(cloudBackend);
  const recognitionProfiles = apiProfiles.profiles.filter(
    (profile) => profile.provider === provider && supportsRecognition(profile),
  );
  const selectedProfile = recognitionProfiles.find((profile) => profile.id === selectedProfileId);
  const selectedModel = asr.managedModels.find((model) => model.id === draftController.draft.asr.local.model);
  const localSettingsError = asrSelectionError(draftController.draft, asrCapabilities, (key) => t(key));
  const localReady = Boolean(
    selectedModel
    && ["downloaded", "loading", "ready"].includes(selectedModel.status)
    && !localSettingsError
    && draftController.saveState !== "error"
  );
  const deviceErrors = devicesReady ? audioSelectionErrors(settings, devices, (key) => t(key)) : [];
  const audioReady = devicesReady
    && deviceErrors.length === 0
    && hasEnabledAudioSource(settings);
  const saveBusy = draftController.saveState === "saving";
  const microphoneOperationBusy = microphoneTestBusy || calibration.calibrating;
  const operationBusy = busy || audioBusy || microphoneOperationBusy || saveBusy || apiProfiles.busy !== null;

  const stepItems = useMemo(() => [
    { icon: <Sparkles size={16} />, label: t("onboarding.steps.welcome") },
    { icon: <AudioLines size={16} />, label: t("onboarding.steps.recognition") },
    { icon: <Volume2 size={16} />, label: t("onboarding.steps.audio") },
    { icon: <Mic size={16} />, label: t("onboarding.steps.microphone") },
    { icon: <Check size={16} />, label: t("onboarding.steps.complete") },
  ], [t]);

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

  useEffect(() => {
    if (step === 3 || !microphoneStartedByWizardRef.current) return;
    calibration.reset();
    microphoneStartedByWizardRef.current = false;
    void onStopMicrophoneTest().catch(() => undefined);
  }, [calibration.reset, onStopMicrophoneTest, step]);

  useEffect(() => () => {
    if (microphoneStartedByWizardRef.current) {
      void onStopMicrophoneTest().catch(() => undefined);
    }
  }, [onStopMicrophoneTest]);

  const showError = (reason: unknown, fallbackKey = "errors.operation") => {
    setMessageTone("error");
    setMessage(localizedError(reason, t, fallbackKey));
  };

  const goToStep = async (next: number) => {
    if (progressPendingRef.current) return;
    progressPendingRef.current = true;
    setBusy(true);
    setMessage("");
    try {
      await onProgress(next);
      setStep(next);
    } catch (reason) {
      showError(reason, "onboarding.errors.progress");
    } finally {
      progressPendingRef.current = false;
      setBusy(false);
    }
  };

  const updateLanguage = async (preference: UiLanguagePreference) => {
    const previous = languagePreference;
    setLanguagePreference(preference);
    setBusy(true);
    setMessage("");
    try {
      await changeUiLanguage(preference);
    } catch (reason) {
      setLanguagePreference(previous);
      showError(reason, "errors.desktop.language");
    } finally {
      setBusy(false);
    }
  };

  const saveApiEditor = async () => {
    if (!apiEditor) return;
    setMessage("");
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
    setMessage("");
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
      setMessageTone("info");
      setMessage(t("onboarding.recognition.connectionReady"));
    } catch (reason) {
      showError(reason, "errors.apiProfiles.operation");
    } finally {
      setBusy(false);
    }
  };

  const finishRecognition = async () => {
    if (recognitionMode === "cloud") {
      if (!selectedProfile || testedProfileId !== selectedProfile.id) return;
      await goToStep(2);
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
      await goToStep(2);
    } catch (reason) {
      showError(reason, "errors.settings.apply");
    } finally {
      setBusy(false);
    }
  };

  const updateSettings = async (update: (current: Settings) => Settings) => {
    setAudioBusy(true);
    setMessage("");
    try {
      await onSave(update(settings));
    } catch (reason) {
      showError(reason, "errors.settings.apply");
    } finally {
      setAudioBusy(false);
    }
  };

  const toggleMicrophoneTest = async () => {
    if (microphoneOperationBusy) return;
    calibration.reset();
    setMicrophoneTestBusy(true);
    setMessage("");
    try {
      if (health?.microphone_test_running) {
        await onStopMicrophoneTest();
        microphoneStartedByWizardRef.current = false;
      } else {
        await onStartMicrophoneTest();
        microphoneStartedByWizardRef.current = true;
        setMicrophoneReviewed(true);
      }
    } catch (reason) {
      showError(reason, "errors.audio.microphoneTestFailed");
    } finally {
      setMicrophoneTestBusy(false);
    }
  };

  const startCalibration = async () => {
    if (microphoneOperationBusy) return;
    setMessage("");
    try {
      await calibration.start();
      setMicrophoneReviewed(true);
    } catch (reason) {
      showError(reason, "errors.audio.microphoneTestFailed");
    }
  };

  const applySuggestedThreshold = async () => {
    if (!calibration.result) return;
    const threshold = calibration.result.threshold;
    await updateSettings((current) => ({
      ...current,
      audio: {
        ...current.audio,
        microphone: {
          ...current.audio.microphone,
          trigger_threshold_dbfs: threshold,
        },
      },
    }));
    setMicrophoneReviewed(true);
  };

  const leaveWizard = async () => {
    setBusy(true);
    setMessage("");
    try {
      calibration.reset();
      if (microphoneStartedByWizardRef.current || health?.microphone_test_running) {
        await onStopMicrophoneTest();
        microphoneStartedByWizardRef.current = false;
      }
      await onComplete(false);
    } catch (reason) {
      showError(reason, "onboarding.errors.finish");
      setBusy(false);
    }
  };

  const skipWizard = async () => {
    setBusy(true);
    setMessage("");
    try {
      calibration.reset();
      if (microphoneStartedByWizardRef.current || health?.microphone_test_running) {
        await onStopMicrophoneTest();
        microphoneStartedByWizardRef.current = false;
      }
      await onSkip();
    } catch (reason) {
      showError(reason, "onboarding.errors.finish");
      setBusy(false);
    }
  };

  const canContinue = step === 0
    ? !operationBusy
    : step === 1
      ? recognitionMode === "local"
        ? localReady && !operationBusy
        : Boolean(selectedProfile?.credential.configured && testedProfileId === selectedProfile.id) && !operationBusy
      : step === 2
        ? audioReady && !operationBusy
        : step === 3
          ? audioReady
            && (settings.audio.microphone.mode === "disabled" || microphoneReviewed)
            && !operationBusy
          : !operationBusy;

  return (
    <div className="onboarding-scroll-region">
      <section className="onboarding-surface" aria-label={t("onboarding.title")}>
        <aside className="onboarding-sidebar">
          <div className="onboarding-brand">
            <img src="/logos/VRCS_Logo.svg" alt="VRCS" />
          </div>
          <ol className="onboarding-progress">
            {stepItems.map((item, index) => (
              <li className={`${index === step ? "current" : ""} ${index < step ? "complete" : ""}`} aria-current={index === step ? "step" : undefined} key={item.label}>
                <span>{index < step ? <Check size={15} /> : item.icon}</span>
                <div><small>{t("onboarding.stepCounter", { current: index + 1, total: STEP_COUNT })}</small><strong>{item.label}</strong></div>
              </li>
            ))}
          </ol>
          <p className="onboarding-save-note">{t("onboarding.saveImmediately")}</p>
        </aside>

        <div className="onboarding-main">
          <header className="onboarding-header">
            <div>
              <small>{t("onboarding.stepCounter", { current: step + 1, total: STEP_COUNT })}</small>
              <h1>{stepItems[step].label}</h1>
            </div>
            {step < STEP_COUNT - 1 && (
              <button className="onboarding-skip-button" type="button" disabled={operationBusy} onClick={() => void skipWizard()}>
                {t("onboarding.skip")}
              </button>
            )}
          </header>

          <div className="onboarding-content" key={step}>
            {step === 0 && (
              <div className="onboarding-welcome">
                <h2>{t("onboarding.welcome.title")}</h2>
                <p>{t("onboarding.welcome.description")}</p>
                <div className="onboarding-info-grid">
                  <article><ShieldCheck size={19} /><strong>{t("onboarding.welcome.privacyTitle")}</strong><span>{t("onboarding.welcome.privacyDescription")}</span></article>
                  <article><AudioLines size={19} /><strong>{t("onboarding.welcome.realtimeTitle")}</strong><span>{t("onboarding.welcome.realtimeDescription")}</span></article>
                </div>
                <div className="onboarding-language-setting">
                  <div className="onboarding-language-copy"><Languages size={18} /><span><strong>{t("settings.system.language")}</strong><small>{t("settings.system.languageDescription")}</small></span></div>
                  <Select
                    label={t("settings.system.language")}
                    value={languagePreference}
                    options={[
                      { value: "system", label: t("settings.system.followSystem") },
                      ...localeCatalog.map(({ _meta }) => ({ value: _meta.locale, label: _meta.name })),
                    ]}
                    disabled={busy}
                    hideLabel
                    onChange={(value) => void updateLanguage(value as UiLanguagePreference)}
                  />
                </div>
              </div>
            )}

            {step === 1 && (
              <div className="onboarding-step-content">
                <div className="onboarding-intro"><p>{t("onboarding.recognition.description")}</p></div>
                <div className="onboarding-choice-grid">
                  <button className={`onboarding-choice ${recognitionMode === "cloud" ? "selected" : ""}`} type="button" aria-pressed={recognitionMode === "cloud"} disabled={operationBusy} onClick={() => setRecognitionMode("cloud")}>
                    <Cloud size={23} /><span><strong>{t("onboarding.recognition.cloud")}</strong><small>{t("onboarding.recognition.cloudDescription")}</small></span><i>{recognitionMode === "cloud" && <Check size={14} />}</i>
                  </button>
                  <button className={`onboarding-choice ${recognitionMode === "local" ? "selected" : ""}`} type="button" aria-pressed={recognitionMode === "local"} disabled={operationBusy} onClick={() => setRecognitionMode("local")}>
                    <HardDrive size={23} /><span><strong>{t("onboarding.recognition.local")}</strong><small>{t("onboarding.recognition.localDescription")}</small></span><i>{recognitionMode === "local" && <Check size={14} />}</i>
                  </button>
                </div>

                {recognitionMode === "cloud" ? (
                  <div className="onboarding-config-panel">
                    <div className="onboarding-panel-heading"><KeyRound size={18} /><div><strong>{t("onboarding.recognition.cloudSetup")}</strong><small>{t("settings.apiManagement.securityNotice")}</small></div></div>
                    <Select
                      label={t("settings.recognition.cloudService")}
                      value={cloudBackend}
                      options={[
                        { value: "qwen_realtime", label: "Alibaba Cloud · Qwen3 ASR" },
                        { value: "fun_asr_realtime", label: "Alibaba Cloud · Fun-ASR" },
                        { value: "openai_realtime", label: "OpenAI Realtime" },
                      ]}
                      disabled={operationBusy}
                      onChange={(value) => {
                        setCloudBackend(value as CloudBackend);
                        setSelectedProfileId("");
                        setTestedProfileId("");
                        setApiEditor(null);
                      }}
                    />
                    {recognitionProfiles.length > 0 && !apiEditor && (
                      <div className="onboarding-profile-select">
                        <Select
                          label={t("settings.recognition.selectApiProfile")}
                          value={selectedProfileId}
                          options={recognitionProfiles.map((profile) => ({
                            value: profile.id,
                            label: `${profile.name} · ${profile.credential.configured ? t("settings.apiManagement.configured") : t("settings.apiManagement.notConfigured")}`,
                          }))}
                          disabled={operationBusy}
                          onChange={(value) => { setSelectedProfileId(value); setTestedProfileId(""); }}
                        />
                        <button className="secondary-button" type="button" disabled={operationBusy} onClick={() => setApiEditor(createApiProfileDraft(provider))}>
                          {t("onboarding.recognition.addAnother")}
                        </button>
                      </div>
                    )}
                    {!apiEditor && recognitionProfiles.length === 0 && (
                      <button className="onboarding-empty-action" type="button" disabled={operationBusy || apiProfiles.loading} onClick={() => setApiEditor(createApiProfileDraft(provider))}>
                        <KeyRound size={19} /><span><strong>{t("onboarding.recognition.addApi")}</strong><small>{t("onboarding.recognition.addApiDescription")}</small></span><ChevronRight size={18} />
                      </button>
                    )}
                    {apiEditor && (
                      <ApiProfileEditor
                        draft={apiEditor}
                        saving={apiProfiles.busy === "create"}
                        providers={[provider]}
                        purposes={["asr", "shared"]}
                        requireCredential
                        onChange={setApiEditor}
                        onSave={() => void saveApiEditor()}
                        onCancel={() => setApiEditor(null)}
                      />
                    )}
                    {selectedProfile && !apiEditor && (
                      <div className={`onboarding-connection ${testedProfileId === selectedProfile.id ? "ready" : ""}`}>
                        <span className="recognition-runtime-dot" />
                        <div><strong>{selectedProfile.name}</strong><small>{testedProfileId === selectedProfile.id ? t("onboarding.recognition.connectionReady") : t("onboarding.recognition.testRequired")}</small></div>
                        <button className="primary-button" type="button" disabled={operationBusy || !selectedProfile.credential.configured} onClick={() => void testAndApplyCloud()}>
                          {busy ? <RefreshCw className="spin" size={15} /> : <ShieldCheck size={15} />}
                          {t("settings.apiManagement.testAsr")}
                        </button>
                      </div>
                    )}
                    {apiProfiles.message && <p className="onboarding-feedback" role="status">{apiProfiles.message}</p>}
                  </div>
                ) : (
                  <div className="onboarding-local-panel">
                    <LocalRuntimeStatus capabilities={asrCapabilities} />
                    <LocalRecognitionSettings
                      draft={draftController.draft}
                      disabled={operationBusy}
                      capabilities={asrCapabilities}
                      asrError={localSettingsError}
                      modelStatusLabel={asr.modelStatusLabel}
                      computeTypes={validComputeTypes(asrCapabilities, draftController.draft.asr.local.device)}
                      selectableModels={asr.selectable}
                      onUpdateAsr={asr.updateAsr}
                      onUpdateLocalAsr={asr.updateLocalAsr}
                    />
                    <ModelManagerPanel
                      locale={locale}
                      disabled={operationBusy}
                      installedModels={asr.installed}
                      downloadingModels={asr.downloading}
                      managedModels={asr.managedModels}
                      modelsReady={asr.modelsReady}
                      message={asr.message}
                      directoryText={asr.modelDirectoryText}
                      saveState={draftController.saveState}
                      onLoad={asr.loadModels}
                      onSetDirectoryText={asr.setModelDirectoryText}
                      onUpdateDirectory={asr.updateModelDirectory}
                      onChooseDirectory={asr.chooseModelDirectory}
                      onDownload={asr.downloadModel}
                      onRemove={asr.removeModel}
                    />
                    {!localReady && <p className="onboarding-feedback">{t("onboarding.recognition.downloadRequired")}</p>}
                  </div>
                )}
              </div>
            )}

            {step === 2 && (
              <div className="onboarding-step-content">
                <div className="onboarding-section-heading">
                  <div><p>{t("onboarding.audio.description")}</p></div>
                  <button className="secondary-button" type="button" disabled={audioBusy} onClick={() => void onRefreshDevices()}><RefreshCw size={15} />{t("settings.audio.rescan")}</button>
                </div>
                {deviceErrors.map((error) => <p className="settings-validation-error" role="alert" key={error}>{error}</p>)}
                {devicesReady && !hasEnabledAudioSource(settings) && (
                  <p className="settings-validation-error" role="alert">{t("onboarding.audio.sourceRequired")}</p>
                )}
                <DeviceGroup
                  icon={<Volume2 size={18} />}
                  title={t("settings.audio.otherVoice")}
                  note={t("settings.audio.otherVoiceDescription")}
                  devices={outputDevices}
                  devicesReady={devicesReady}
                  selectedDeviceId={settings.audio.output.mode === "system" ? settings.audio.output.device_id : null}
                  specialRows={[
                    { key: "system", name: t("settings.audio.systemOutput"), description: t("settings.audio.systemOutputDescription"), chosen: settings.audio.output.mode === "system" && settings.audio.output.device_id === null, onSelect: () => void updateSettings((current) => ({ ...current, audio: { ...current.audio, output: { mode: "system", device_id: null } } })) },
                    { key: "vrchat", name: "VRChat", description: t("settings.audio.vrchatDescription"), chosen: settings.audio.output.mode === "vrchat", onSelect: () => void updateSettings((current) => ({ ...current, audio: { ...current.audio, output: { mode: "vrchat", device_id: null } } })) },
                    { key: "disabled", name: t("settings.audio.disableOtherVoices"), description: t("settings.audio.disableOtherVoicesDescription"), chosen: settings.audio.output.mode === "disabled", onSelect: () => void updateSettings((current) => ({ ...current, audio: { ...current.audio, output: { mode: "disabled", device_id: null } } })) },
                  ]}
                  disabled={audioBusy}
                  onSelectDevice={(id) => void updateSettings((current) => ({ ...current, audio: { ...current.audio, output: { mode: "system", device_id: id } } }))}
                />
                <DeviceGroup
                  icon={<Mic size={18} />}
                  title={t("settings.audio.ownVoice")}
                  note={t("settings.audio.ownVoiceDescription")}
                  devices={microphoneDevices}
                  devicesReady={devicesReady}
                  selectedDeviceId={settings.audio.microphone.mode === "device" ? settings.audio.microphone.device_id : null}
                  specialRows={[
                    { key: "default", name: t("settings.audio.defaultMicrophone"), description: t("settings.audio.defaultMicrophoneDescription"), chosen: settings.audio.microphone.mode === "default", onSelect: () => { setMicrophoneReviewed(false); void updateSettings((current) => ({ ...current, audio: { ...current.audio, microphone: { ...current.audio.microphone, mode: "default", device_id: null } } })); } },
                    { key: "disabled", name: t("settings.audio.disableMicrophone"), description: t("settings.audio.disableMicrophoneDescription"), chosen: settings.audio.microphone.mode === "disabled", onSelect: () => { setMicrophoneReviewed(true); void updateSettings((current) => ({ ...current, audio: { ...current.audio, microphone: { ...current.audio.microphone, mode: "disabled", device_id: null } } })); } },
                  ]}
                  disabled={audioBusy}
                  onSelectDevice={(id) => { setMicrophoneReviewed(false); void updateSettings((current) => ({ ...current, audio: { ...current.audio, microphone: { ...current.audio.microphone, mode: "device", device_id: id } } })); }}
                />
              </div>
            )}

            {step === 3 && (
              <div className="onboarding-step-content">
                {settings.audio.microphone.mode === "disabled" ? (
                  <div className="onboarding-disabled-feature"><Mic size={28} /><h2>{t("onboarding.microphone.disabledTitle")}</h2><p>{t("onboarding.microphone.disabledDescription")}</p></div>
                ) : (
                  <>
                    <div className="onboarding-intro"><p>{t("onboarding.microphone.description")}</p></div>
                    <MicrophoneLevelSetting
                      level={microphoneLevel}
                      enabled
                      captureRunning={false}
                      transcriptionRunning={health?.capture_requested ?? false}
                      testing={health?.microphone_test_running ?? false}
                      busy={microphoneOperationBusy}
                      threshold={settings.audio.microphone.trigger_threshold_dbfs}
                      disabled={audioBusy}
                      onToggleTest={() => void toggleMicrophoneTest()}
                      onCommit={(threshold) => { setMicrophoneReviewed(true); void updateSettings((current) => ({ ...current, audio: { ...current.audio, microphone: { ...current.audio.microphone, trigger_threshold_dbfs: threshold } } })); }}
                    />
                    <div className="onboarding-calibration-card">
                      <div className="onboarding-panel-heading"><Sparkles size={18} /><div><strong>{t("onboarding.microphone.autoTitle")}</strong><small>{t("onboarding.microphone.autoDescription")}</small></div></div>
                      <div className={`onboarding-calibration-status phase-${calibration.phase}`} role="status" aria-live="polite">
                        <span><i /></span>
                        <div>
                          <strong>{t(`onboarding.microphone.phase.${calibration.phase}`)}</strong>
                          <small>{calibration.phase === "ready" && calibration.result
                            ? t("onboarding.microphone.result", { noise: calibration.result.noiseLevel, speech: calibration.result.speechLevel, threshold: calibration.result.threshold })
                            : t(`onboarding.microphone.phaseDescription.${calibration.phase}`)}</small>
                        </div>
                      </div>
                      <div className="onboarding-calibration-actions">
                        <button className="secondary-button" type="button" disabled={microphoneOperationBusy || audioBusy} onClick={() => { setMicrophoneReviewed(true); calibration.reset(); void updateSettings((current) => ({ ...current, audio: { ...current.audio, microphone: { ...current.audio.microphone, trigger_threshold_dbfs: DEFAULT_MICROPHONE_THRESHOLD_DBFS } } })); }}>{t("onboarding.microphone.restoreDefault")}</button>
                        {calibration.result && <button className="secondary-button" type="button" disabled={audioBusy} onClick={() => void applySuggestedThreshold()}>{t("onboarding.microphone.applySuggestion")}</button>}
                        <button className="primary-button" type="button" disabled={microphoneOperationBusy || audioBusy} onClick={() => void startCalibration()}>{microphoneOperationBusy ? <RefreshCw className="spin" size={15} /> : <Sparkles size={15} />}{calibration.phase === "idle" ? t("onboarding.microphone.startCalibration") : t("onboarding.microphone.retryCalibration")}</button>
                      </div>
                    </div>
                    {!microphoneReviewed && <button className="onboarding-text-action" type="button" onClick={() => setMicrophoneReviewed(true)}>{t("onboarding.microphone.skipCalibration")}</button>}
                  </>
                )}
              </div>
            )}

            {step === 4 && (
              <div className="onboarding-complete">
                <div className="onboarding-complete-icon"><Check size={34} /></div>
                <h2>{t("onboarding.complete.title")}</h2>
                <p>{t("onboarding.complete.description")}</p>
                <div className="onboarding-checklist">
                  <article><Check size={16} /><span><strong>{t("onboarding.complete.recognition")}</strong><small>{recognitionMode === "local" ? t("onboarding.complete.localReady") : t("onboarding.complete.cloudReady")}</small></span></article>
                  <article>{audioReady ? <Check size={16} /> : <Volume2 size={16} />}<span><strong>{t("onboarding.complete.audio")}</strong><small>{t(audioReady ? "onboarding.complete.audioReady" : "onboarding.complete.audioNotReady")}</small></span></article>
                  <article><Check size={16} /><span><strong>{t("onboarding.complete.microphone")}</strong><small>{settings.audio.microphone.mode === "disabled" ? t("onboarding.complete.microphoneSkipped") : t("onboarding.complete.microphoneReady")}</small></span></article>
                </div>
                <div className="onboarding-next-hints"><span>{t("onboarding.complete.nextHint")}</span><div><span>{t("settings.categories.translation")}</span><span>{t("settings.categories.learning")}</span><span>{t("settings.categories.connections")}</span></div></div>
              </div>
            )}
          </div>

          {message && <p className={`onboarding-message ${messageTone}`} role={messageTone === "error" ? "alert" : "status"}>{message}</p>}
          {draftController.saveMessage && <p className="onboarding-message error" role="alert">{draftController.saveMessage}</p>}

          <footer className="onboarding-actions">
            <div>
              {step > 0 && <button className="secondary-button" type="button" disabled={operationBusy} onClick={() => void goToStep(step - 1)}><ChevronLeft size={16} />{t("onboarding.back")}</button>}
            </div>
            <div>
              {step < STEP_COUNT - 1 ? (
                <button className="primary-button" type="button" disabled={!canContinue} onClick={() => step === 1 ? void finishRecognition() : void goToStep(step + 1)}>{t("onboarding.next")}<ChevronRight size={16} /></button>
              ) : (
                <button className="primary-button" type="button" disabled={operationBusy || !audioReady} onClick={() => void leaveWizard()}>{t("onboarding.complete.finish")}<ChevronRight size={16} /></button>
              )}
            </div>
          </footer>
        </div>
      </section>
    </div>
  );
}
