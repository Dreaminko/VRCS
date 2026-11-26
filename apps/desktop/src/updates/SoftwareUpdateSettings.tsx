import { ExternalLink, RefreshCw } from "lucide-react";
import { useTranslation } from "react-i18next";

import { PreferenceToggle } from "../settings/SettingsControls";
import { openReleasePage } from "./app-updater";
import { UpdateDownloadProgress } from "./UpdateDownloadProgress";
import {
  extractNotesPreview,
  formatLastCheckedTime,
  isNightlyVersion,
  resolveUpdateErrorKey,
} from "./update-notes";
import type { AppUpdaterState } from "./useAppUpdater";

function updateStatusKey(updater: AppUpdaterState): string {
  if (updater.phase === "error") return `updates.errors.${resolveUpdateErrorKey(updater.errorCode)}`;
  if (!updater.buildInfo?.updaterAvailable) return "updates.status.unavailable";
  if (updater.phase === "checking") return "updates.status.checking";
  if (updater.phase === "upToDate") return "updates.status.upToDate";
  if (updater.phase === "available") return "updates.status.available";
  if (updater.phase === "downloading") return "updates.status.downloading";
  if (updater.phase === "installing") return "updates.status.installing";
  return "updates.status.ready";
}

export function SoftwareUpdateSettings({
  updater,
  transcriptionRunning = false,
}: {
  updater: AppUpdaterState;
  transcriptionRunning?: boolean;
}) {
  const { t, i18n } = useTranslation();
  const busy = updater.preferenceSaving || updater.phase === "checking"
    || updater.phase === "downloading"
    || updater.phase === "installing";
  const statusKey = updateStatusKey(updater);
  const canInstall = updater.buildInfo?.canInstall ?? false;
  const currentIsNightly = isNightlyVersion(updater.buildInfo?.version);
  const lastCheckedText = formatLastCheckedTime(updater.lastCheckedAt, i18n.language);
  const available = updater.update;
  const previewItems = available ? extractNotesPreview(updater.liveNotes ?? available.notes, 2) : [];
  const isSkipped = Boolean(available && updater.skippedVersion === available.version);

  return (
    <section className="system-settings-group software-update-settings" aria-labelledby="software-update-title">
      <div className="section-heading">
        <div><RefreshCw size={18} /><h3 id="software-update-title">{t("updates.title")}</h3></div>
      </div>

      <div className="software-update-summary">
        <div>
          <div className="software-update-version-row">
            <strong>{t("updates.currentVersion", { version: updater.buildInfo?.version ?? "—" })}</strong>
            {currentIsNightly && (
              <span className="update-channel-badge nightly">{t("updates.channel.nightly")}</span>
            )}
          </div>
          <small>
            {t(`updates.variant.${updater.buildInfo?.variant ?? "standard"}`)}
            {lastCheckedText ? ` · ${t("updates.lastChecked", { time: lastCheckedText })}` : ""}
          </small>
          <p className={updater.phase === "error" ? "error" : ""}>{t(statusKey, {
            version: available?.version,
            channel: available ? t(`updates.channel.${available.channel}`) : undefined,
          })}</p>
          {updater.buildInfo?.os === "linux" && updater.buildInfo.canInstall === false && (
            <p role="status">{t("platform.manual_updates")}</p>
          )}
        </div>
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

      {available && (
        <div className="software-update-available">
          <div className="software-update-available-header">
            <div className="software-update-available-title">
              <span className={`update-channel-badge ${available.channel}`}>
                {t(`updates.channel.${available.channel}`)}
              </span>
              <strong>
                {t("updates.versionTransition", {
                  current: available.currentVersion,
                  next: available.version,
                })}
              </strong>
              {isSkipped && (
                <span className="software-update-skipped-pill">{t("updates.versionSkipped")}</span>
              )}
            </div>
          </div>

          {previewItems.length > 0 && (
            <ul className="update-notice-preview">
              {previewItems.map((item) => (
                <li key={item}>{item}</li>
              ))}
            </ul>
          )}

          <UpdateDownloadProgress updater={updater} />

          {canInstall && transcriptionRunning && updater.phase !== "downloading" && updater.phase !== "installing" && (
            <small className="software-update-warning">{t("updates.stopTranscriptionFirst")}</small>
          )}

          <div className="software-update-available-actions">
            <button
              className="primary-button"
              type="button"
              disabled={busy || (canInstall && transcriptionRunning)}
              onClick={() => void (updater.phase === "error"
                ? updater.retry()
                : canInstall
                  ? updater.install()
                  : openReleasePage(available.releaseUrl))}
            >
              {t(updater.phase === "error"
                ? "common.retry"
                : canInstall
                  ? "updates.downloadAndInstall"
                  : "updates.openReleasePage")}
            </button>
            <button
              className="secondary-button"
              type="button"
              onClick={updater.openDetails}
            >
              {t("updates.viewDetails")}
            </button>
            {canInstall && (
              <button
                className="update-inline-link"
                type="button"
                onClick={() => void openReleasePage(available.releaseUrl)}
              >
                <ExternalLink size={12} aria-hidden="true" />
                {t("updates.viewOnGitHub")}
              </button>
            )}
          </div>
        </div>
      )}

      <PreferenceToggle
        title={t("updates.automaticChecks")}
        checked={updater.automaticChecks}
        disabled={!updater.buildInfo?.updaterAvailable || !updater.preferenceReady || busy}
        onChange={(enabled) => void updater.setAutomaticChecks(enabled)}
      />
      {updater.buildInfo?.os !== "linux" && (
        <>
          <PreferenceToggle
            title={t("updates.receiveNightlyUpdates")}
            description={t("updates.nightlyDescription")}
            checked={updater.receiveNightlyUpdates}
            disabled={!updater.buildInfo?.updaterAvailable || !updater.preferenceReady || busy}
            onChange={(enabled) => void updater.setReceiveNightlyUpdates(enabled)}
          />
          {currentIsNightly && !updater.receiveNightlyUpdates && (
            <p className="software-update-nightly-note">{t("updates.nightlyWaitStableHint")}</p>
          )}
        </>
      )}
      {updater.preferenceError && (
        <p className="error" role="alert">
          {t(`updates.errors.${resolveUpdateErrorKey(updater.preferenceError)}`)}
        </p>
      )}
    </section>
  );
}
