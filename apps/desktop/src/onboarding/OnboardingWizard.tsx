import { useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  AudioLines,
  Check,
  ChevronLeft,
  ChevronRight,
  Mic,
  Sparkles,
  Volume2,
} from "lucide-react";

import { localizedError } from "../app/app-utils";
import type { UiLanguagePreference } from "../app/ui-language";
import { changeUiLanguage, currentUiLanguagePreference } from "../i18n";
import type { AsrCapabilities, AudioDevice, Health, Settings } from "../types";
import { useOnboardingAudio } from "./hooks/useOnboardingAudio";
import { useOnboardingMicrophone } from "./hooks/useOnboardingMicrophone";
import { useOnboardingRecognition } from "./hooks/useOnboardingRecognition";
import { AudioStep } from "./steps/AudioStep";
import { CompleteStep } from "./steps/CompleteStep";
import { MicrophoneStep } from "./steps/MicrophoneStep";
import { RecognitionStep } from "./steps/RecognitionStep";
import { WelcomeStep } from "./steps/WelcomeStep";

const STEP_COUNT = 5;

export function OnboardingWizard({
  initialStep,
  settings,
  health,
  devices,
  devicesReady,
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
  const progressPendingRef = useRef(false);

  const showError = (reason: unknown, fallbackKey = "errors.operation") => {
    setMessageTone("error");
    setMessage(localizedError(reason, t, fallbackKey));
  };
  const showInfo = (nextMessage: string) => {
    setMessageTone("info");
    setMessage(nextMessage);
  };
  const clearMessage = () => setMessage("");

  const goToStep = async (next: number) => {
    if (progressPendingRef.current) return;
    progressPendingRef.current = true;
    setBusy(true);
    clearMessage();
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

  const recognition = useOnboardingRecognition({
    active: step === 1,
    settings,
    asrCapabilities,
    modelStatus,
    onRefreshSettings,
    onModelsChanged,
    onSave,
    onFinish: () => goToStep(2),
    clearMessage,
    showError,
    showInfo,
  });
  const audio = useOnboardingAudio({
    settings,
    devices,
    devicesReady,
    onSave,
    clearMessage,
    showError,
  });
  const microphone = useOnboardingMicrophone({
    step,
    settings,
    health,
    updateSettings: audio.updateSettings,
    onStartMicrophoneTest,
    onStopMicrophoneTest,
    clearMessage,
    showError,
  });
  const operationBusy = busy
    || recognition.busy
    || audio.busy
    || microphone.operationBusy
    || recognition.saveBusy
    || recognition.apiProfiles.busy !== null;

  const stepItems = useMemo(() => [
    { icon: <Sparkles size={16} />, label: t("onboarding.steps.welcome") },
    { icon: <AudioLines size={16} />, label: t("onboarding.steps.recognition") },
    { icon: <Volume2 size={16} />, label: t("onboarding.steps.audio") },
    { icon: <Mic size={16} />, label: t("onboarding.steps.microphone") },
    { icon: <Check size={16} />, label: t("onboarding.steps.complete") },
  ], [t]);

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

  const leaveWizard = async () => {
    setBusy(true);
    setMessage("");
    try {
      await microphone.stopAndReset();
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
      await microphone.stopAndReset();
      await onSkip();
    } catch (reason) {
      showError(reason, "onboarding.errors.finish");
      setBusy(false);
    }
  };

  const canContinue = step === 0
    ? !operationBusy
    : step === 1
      ? recognition.recognitionMode === "local"
        ? recognition.localReady && !operationBusy
        : Boolean(
          recognition.selectedProfile?.credential.configured
          && recognition.cloudReady,
        ) && !operationBusy
      : step === 2
        ? audio.ready && !operationBusy
        : step === 3
          ? audio.ready
            && (settings.audio.microphone.mode === "disabled" || microphone.reviewed)
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
              <WelcomeStep
                languagePreference={languagePreference}
                busy={busy}
                onUpdateLanguage={(preference) => void updateLanguage(preference)}
              />
            )}

            {step === 1 && (
              <RecognitionStep
                recognitionMode={recognition.recognitionMode}
                operationBusy={operationBusy}
                recognitionProfiles={recognition.recognitionProfiles}
                recognitionServices={recognition.recognitionServices}
                selectedProfileId={recognition.selectedProfileId}
                selectedServiceId={recognition.selectedServiceId}
                testedSelectionId={recognition.testedSelectionId}
                selectedProfile={recognition.selectedProfile}
                selectedService={recognition.selectedService}
                apiEditor={recognition.apiEditor}
                apiProfiles={recognition.apiProfiles}
                draftController={recognition.draftController}
                asr={recognition.asr}
                asrCapabilities={asrCapabilities}
                localSettingsError={recognition.localSettingsError}
                localReady={recognition.localReady}
                locale={locale}
                busy={recognition.busy}
                onSetRecognitionMode={recognition.setRecognitionMode}
                onSelectProfile={recognition.selectProfile}
                onSelectService={recognition.selectService}
                onAddApiProfile={recognition.addApiProfile}
                onChangeApiEditor={recognition.setApiEditor}
                onSaveApiEditor={() => void recognition.saveApiEditor()}
                onCancelApiEditor={recognition.cancelApiEditor}
                onTestAndApplyCloud={() => void recognition.testAndApplyCloud()}
              />
            )}

            {step === 2 && (
              <AudioStep
                settings={settings}
                outputDevices={audio.outputDevices}
                microphoneDevices={audio.microphoneDevices}
                devicesReady={devicesReady}
                deviceErrors={audio.deviceErrors}
                hasAudioSource={audio.hasAudioSource}
                audioBusy={audio.busy}
                onRefreshDevices={() => void onRefreshDevices()}
                onUpdateSettings={(update) => void audio.updateSettings(update)}
                onSetMicrophoneReviewed={microphone.setReviewed}
              />
            )}

            {step === 3 && (
              <MicrophoneStep
                settings={settings}
                health={health}
                microphoneLevel={microphone.microphoneLevel}
                microphoneOperationBusy={microphone.operationBusy}
                audioBusy={audio.busy}
                microphoneReviewed={microphone.reviewed}
                calibrationPhase={microphone.calibration.phase}
                calibrationResult={microphone.calibration.result}
                onToggleMicrophoneTest={() => void microphone.toggleTest()}
                onCommitThreshold={microphone.commitThreshold}
                onRestoreDefault={microphone.restoreDefault}
                onApplySuggestedThreshold={() => void microphone.applySuggestedThreshold()}
                onStartCalibration={() => void microphone.startCalibration()}
                onSkipCalibration={microphone.skipCalibration}
              />
            )}

            {step === 4 && (
              <CompleteStep
                recognitionMode={recognition.recognitionMode}
                audioReady={audio.ready}
                microphoneDisabled={settings.audio.microphone.mode === "disabled"}
              />
            )}
          </div>

          {message && <p className={`onboarding-message ${messageTone}`} role={messageTone === "error" ? "alert" : "status"}>{message}</p>}
          {recognition.draftController.saveMessage && <p className="onboarding-message error" role="alert">{recognition.draftController.saveMessage}</p>}

          <footer className="onboarding-actions">
            <div>
              {step > 0 && <button className="secondary-button" type="button" disabled={operationBusy} onClick={() => void goToStep(step - 1)}><ChevronLeft size={16} />{t("onboarding.back")}</button>}
            </div>
            <div>
              {step < STEP_COUNT - 1 ? (
                <button className="primary-button" type="button" disabled={!canContinue} onClick={() => step === 1 ? void recognition.finishRecognition() : void goToStep(step + 1)}>{t("onboarding.next")}<ChevronRight size={16} /></button>
              ) : (
                <button className="primary-button" type="button" disabled={operationBusy || !audio.ready} onClick={() => void leaveWizard()}>{t("onboarding.complete.finish")}<ChevronRight size={16} /></button>
              )}
            </div>
          </footer>
        </div>
      </section>
    </div>
  );
}
