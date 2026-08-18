import { Check, Volume2 } from "lucide-react";
import { useTranslation } from "react-i18next";

export function CompleteStep({
  recognitionMode,
  audioReady,
  microphoneDisabled,
}: {
  recognitionMode: "cloud" | "local";
  audioReady: boolean;
  microphoneDisabled: boolean;
}) {
  const { t } = useTranslation();

  return (
    <div className="onboarding-complete">
      <div className="onboarding-complete-icon"><Check size={34} /></div>
      <h2>{t("onboarding.complete.title")}</h2>
      <p>{t("onboarding.complete.description")}</p>
      <div className="onboarding-checklist">
        <article><Check size={16} /><span><strong>{t("onboarding.complete.recognition")}</strong><small>{recognitionMode === "local" ? t("onboarding.complete.localReady") : t("onboarding.complete.cloudReady")}</small></span></article>
        <article>{audioReady ? <Check size={16} /> : <Volume2 size={16} />}<span><strong>{t("onboarding.complete.audio")}</strong><small>{t(audioReady ? "onboarding.complete.audioReady" : "onboarding.complete.audioNotReady")}</small></span></article>
        <article><Check size={16} /><span><strong>{t("onboarding.complete.microphone")}</strong><small>{microphoneDisabled ? t("onboarding.complete.microphoneSkipped") : t("onboarding.complete.microphoneReady")}</small></span></article>
      </div>
      <div className="onboarding-next-hints"><span>{t("onboarding.complete.nextHint")}</span><div><span>{t("settings.categories.translation")}</span><span>{t("settings.categories.learning")}</span><span>{t("settings.categories.connections")}</span></div></div>
    </div>
  );
}
