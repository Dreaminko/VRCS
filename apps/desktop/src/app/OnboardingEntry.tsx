import { lazy, Suspense } from "react";
import { useTranslation } from "react-i18next";

import { WindowChrome } from "../shell/WindowChrome";
import type { AppWorkspace } from "./useAppWorkspace";

const OnboardingWizard = lazy(() => import("../onboarding/OnboardingWizard").then(
  ({ OnboardingWizard }) => ({ default: OnboardingWizard }),
));

export function OnboardingEntry({
  onboarding,
  runtime,
  capture,
  settings,
}: {
  onboarding: AppWorkspace["onboarding"];
  runtime: AppWorkspace["runtime"];
  capture: AppWorkspace["capture"];
  settings: AppWorkspace["settings"];
}) {
  const { t } = useTranslation();

  return (
    <div className="app-shell onboarding-shell">
      <WindowChrome />
      {onboarding.status === "loading" || !settings.value ? (
        <div className="onboarding-loading" role="status" data-tauri-drag-region>
          <span className="onboarding-loading-mark" data-tauri-drag-region>VRCS</span>
          <p data-tauri-drag-region>
            {runtime.startupFailed ? t("errors.core.initialize") : t("common.loading")}
          </p>
          {runtime.startupFailed && (
            <button className="primary-button" type="button" onClick={() => void runtime.retry()}>
              {t("common.retry")}
            </button>
          )}
        </div>
      ) : (
        <Suspense
          fallback={(
            <div className="onboarding-loading" role="status" data-tauri-drag-region>
              <span className="onboarding-loading-mark" data-tauri-drag-region>VRCS</span>
              <p data-tauri-drag-region>{t("common.loading")}</p>
            </div>
          )}
        >
          <OnboardingWizard
            initialStep={onboarding.step}
            settings={settings.value}
            health={runtime.health}
            devices={settings.devices.items}
            devicesReady={settings.devices.ready}
            asrCapabilities={settings.asr.capabilities}
            modelStatus={runtime.health?.asr_status ?? "unknown"}
            onRefreshDevices={settings.devices.refresh}
            onRefreshSettings={settings.refresh}
            onModelsChanged={settings.asr.refresh}
            onStartMicrophoneTest={capture.startMicrophoneTest}
            onStopMicrophoneTest={capture.stopMicrophoneTest}
            onSave={settings.save}
            onProgress={onboarding.saveProgress}
            onSkip={onboarding.skip}
            onComplete={onboarding.finish}
          />
        </Suspense>
      )}
    </div>
  );
}
