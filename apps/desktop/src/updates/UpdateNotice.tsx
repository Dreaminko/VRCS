import { Download, X } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { AppUpdaterState } from "./useAppUpdater";

function formatProgress(downloaded: number, total: number | null): number | null {
  if (!total || total <= 0) return null;
  return Math.min(100, Math.round((downloaded / total) * 100));
}

export function UpdateNotice({
  updater,
  transcriptionRunning,
}: {
  updater: AppUpdaterState;
  transcriptionRunning: boolean;
}) {
  const { t } = useTranslation();
  const progress = formatProgress(updater.downloadedBytes, updater.totalBytes);
  if (!updater.noticeVisible || !updater.update) return null;

  const checking = updater.phase === "checking";
  const busy = updater.phase === "downloading" || updater.phase === "installing";

  return (
    <aside className="update-notice" aria-labelledby="update-notice-title">
      <div className="update-notice-icon" aria-hidden="true"><Download size={18} /></div>
      <div className="update-notice-copy">
        <strong id="update-notice-title">{t("updates.availableTitle", { version: updater.update.version })}</strong>
        <p>{updater.update.notes?.trim() || t("updates.availableDescription")}</p>
        {busy && (
          <div className="update-download-status" role="status">
            <span>{t(updater.phase === "installing" ? "updates.status.installing" : "updates.status.downloading")}</span>
            {progress !== null && <span>{progress}%</span>}
            <div><i style={{ width: progress === null ? "35%" : `${progress}%` }} /></div>
          </div>
        )}
        {transcriptionRunning && !busy && (
          <small>{t("updates.stopTranscriptionFirst")}</small>
        )}
        {updater.phase === "error" && (
          <small className="error">{t(`updates.errors.${updater.errorCode ?? "failed"}`)}</small>
        )}
        <div className="update-notice-actions">
          <button
            className="primary-button"
            type="button"
            disabled={checking || busy || transcriptionRunning}
            onClick={() => void (updater.phase === "error" ? updater.check(true) : updater.install())}
          >
            {t(updater.phase === "error" ? "common.retry" : "updates.downloadAndInstall")}
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
