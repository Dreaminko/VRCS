import { Download, ExternalLink, X } from "lucide-react";
import { useTranslation } from "react-i18next";

import { openReleasePage } from "./app-updater";
import { UpdateDownloadProgress } from "./UpdateDownloadProgress";
import { resolveUpdateErrorKey } from "./update-notes";
import type { AppUpdaterState } from "./useAppUpdater";

export function UpdateNotice({
  updater,
  transcriptionRunning,
  hideError = false,
}: {
  updater: AppUpdaterState;
  transcriptionRunning: boolean;
  hideError?: boolean;
}) {
  const { t } = useTranslation();
  if (!updater.noticeVisible || !updater.update || updater.detailsOpen) return null;

  const { update } = updater;
  const checking = updater.phase === "checking";
  const busy = updater.phase === "downloading" || updater.phase === "installing";
  const canInstall = updater.buildInfo?.canInstall ?? false;
  const errorKey = resolveUpdateErrorKey(updater.errorCode);

  return (
    <aside className="update-notice" aria-labelledby="update-notice-title">
      <div className="update-notice-icon" aria-hidden="true"><Download size={18} /></div>
      <div className="update-notice-copy">
        <div className="update-notice-header">
          <div className="update-notice-title-row">
            <strong id="update-notice-title">
              {t("updates.availableTitle", { version: update.version })}
            </strong>
            <span className={`update-channel-badge ${update.channel}`}>
              {t(`updates.channel.${update.channel}`)}
            </span>
          </div>
          <div className="update-notice-meta">
            <span className="update-version-span">
              {t("updates.versionTransition", {
                current: update.currentVersion,
                next: update.version,
              })}
            </span>
          </div>
        </div>

        <UpdateDownloadProgress updater={updater} />

        {canInstall && transcriptionRunning && !busy && (
          <small>{t("updates.stopTranscriptionFirst")}</small>
        )}

        {!hideError && updater.phase === "error" && (
          <div className="update-notice-error">
            <small className="error">{t(`updates.errors.${errorKey}`)}</small>
            <button
              className="update-inline-link"
              type="button"
              onClick={() => void openReleasePage(update.releaseUrl)}
            >
              <ExternalLink size={12} aria-hidden="true" />
              {t("updates.openReleasePage")}
            </button>
          </div>
        )}

        <div className="update-notice-actions">
          <button
            className="primary-button"
            type="button"
            disabled={checking || busy || updater.preferenceSaving || (canInstall && transcriptionRunning)}
            onClick={() => void (updater.phase === "error"
              ? updater.retry()
              : canInstall
                ? updater.install()
                : openReleasePage(update.releaseUrl))}
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
          {!busy && (
            <button className="secondary-button" type="button" onClick={updater.dismissNotice}>
              {t("updates.later")}
            </button>
          )}
        </div>
      </div>
      {!busy && (
        <button
          className="update-notice-close"
          type="button"
          aria-label={t("common.close")}
          onClick={updater.dismissNotice}
        >
          <X size={16} />
        </button>
      )}
    </aside>
  );
}
