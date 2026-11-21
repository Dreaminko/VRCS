import { RefreshCw } from "lucide-react";
import { useTranslation } from "react-i18next";

import { PreferenceToggle } from "../settings/SettingsControls";
import type { AppUpdaterState } from "./useAppUpdater";

function updateStatusKey(updater: AppUpdaterState): string {
  if (updater.phase === "error") return `updates.errors.${updater.errorCode ?? "failed"}`;
  if (!updater.buildInfo?.updaterAvailable) return "updates.status.unavailable";
  if (updater.phase === "checking") return "updates.status.checking";
  if (updater.phase === "upToDate") return "updates.status.upToDate";
  if (updater.phase === "available") return "updates.status.available";
  if (updater.phase === "downloading") return "updates.status.downloading";
  if (updater.phase === "installing") return "updates.status.installing";
  return "updates.status.ready";
}

export function SoftwareUpdateSettings({ updater }: { updater: AppUpdaterState }) {
  const { t } = useTranslation();
  const busy = updater.preferenceSaving || updater.phase === "checking"
    || updater.phase === "downloading"
    || updater.phase === "installing";
  const statusKey = updateStatusKey(updater);

  return (
    <section className="system-settings-group software-update-settings" aria-labelledby="software-update-title">
      <div className="section-heading">
        <div><RefreshCw size={18} /><h3 id="software-update-title">{t("updates.title")}</h3></div>
      </div>
      <div className="software-update-summary">
        <div>
          <strong>{t("updates.currentVersion", { version: updater.buildInfo?.version ?? "—" })}</strong>
          <small>{t(`updates.variant.${updater.buildInfo?.variant ?? "standard"}`)}</small>
          <p className={updater.phase === "error" ? "error" : ""}>{t(statusKey, {
            version: updater.update?.version,
            channel: updater.update ? t(`updates.channel.${updater.update.channel}`) : undefined,
          })}</p>
        </div>
        {updater.buildInfo?.os === "linux" && updater.buildInfo.canInstall === false && <p role="status">{t("platform.manual_updates")}</p>}
        <button
          className="secondary-button"
          type="button"
          disabled={busy || !updater.buildInfo?.updaterAvailable}
          onClick={() => void updater.check(true)}
        >
          <RefreshCw className={updater.phase === "checking" ? "spin" : ""} size={15} />
          {t("updates.checkNow")}
        </button>
      </div>
      <PreferenceToggle
        title={t("updates.automaticChecks")}
        checked={updater.automaticChecks}
        disabled={!updater.buildInfo?.updaterAvailable || !updater.preferenceReady || busy}
        onChange={(enabled) => void updater.setAutomaticChecks(enabled)}
      />
      {updater.buildInfo?.os !== "linux" && (
        <PreferenceToggle
          title={t("updates.receiveNightlyUpdates")}
          description={t("updates.nightlyDescription")}
          checked={updater.receiveNightlyUpdates}
          disabled={!updater.buildInfo?.updaterAvailable || !updater.preferenceReady || busy}
          onChange={(enabled) => void updater.setReceiveNightlyUpdates(enabled)}
        />
      )}
    </section>
  );
}
