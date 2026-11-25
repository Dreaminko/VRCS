import { useTranslation } from "react-i18next";

import { formatBytes, formatDownloadSpeed } from "./update-notes";
import type { AppUpdaterState } from "./useAppUpdater";

function formatProgress(downloaded: number, total: number | null): number | null {
  if (!total || total <= 0) return null;
  return Math.min(100, Math.round((downloaded / total) * 100));
}

export function UpdateDownloadProgress({ updater }: { updater: AppUpdaterState }) {
  const { t } = useTranslation();
  if (updater.phase !== "downloading" && updater.phase !== "installing") return null;

  const installing = updater.phase === "installing";
  const progress = formatProgress(updater.downloadedBytes, updater.totalBytes);
  const speed = formatDownloadSpeed(updater.downloadSpeedBytesPerSec);

  let metrics = "";
  if (!installing) {
    if (updater.totalBytes && updater.totalBytes > 0 && progress !== null) {
      metrics = t("updates.downloadProgressBytes", {
        downloaded: formatBytes(updater.downloadedBytes),
        total: formatBytes(updater.totalBytes),
      });
      metrics = `${metrics} · ${progress}%`;
      if (speed) metrics = `${metrics} · ${speed}`;
    } else if (updater.downloadedBytes > 0) {
      metrics = t("updates.downloadedBytes", {
        downloaded: formatBytes(updater.downloadedBytes),
      });
      if (speed) metrics = `${metrics} · ${speed}`;
    }
  }

  const indeterminate = !installing && progress === null;

  return (
    <div className="update-download-status" role="status">
      <span>{t(installing ? "updates.status.installingRestart" : "updates.status.downloading")}</span>
      {metrics && <span className="update-download-metrics">{metrics}</span>}
      <div className="update-download-track">
        <i
          className={indeterminate ? "is-indeterminate" : installing ? "is-installing" : undefined}
          style={indeterminate ? undefined : { width: `${installing ? 100 : (progress ?? 0)}%` }}
        />
      </div>
    </div>
  );
}
