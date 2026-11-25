import { ExternalLink } from "lucide-react";
import { useRef } from "react";
import { useTranslation } from "react-i18next";

import { SettingsDialog } from "../settings/components/SettingsDialog";
import { openReleasePage } from "./app-updater";
import { UpdateDownloadProgress } from "./UpdateDownloadProgress";
import { UpdateNotesMarkdown } from "./UpdateNotesMarkdown";
import {
  resolveUpdateErrorKey,
  stripPlatformNotesSection,
} from "./update-notes";
import type { AppUpdaterState } from "./useAppUpdater";

export function UpdateDetailsDialog({
  updater,
  transcriptionRunning,
}: {
  updater: AppUpdaterState;
  transcriptionRunning: boolean;
}) {
  const { t } = useTranslation();
  const capturedFocusRef = useRef<HTMLButtonElement | null>(
    document.activeElement instanceof HTMLButtonElement ? document.activeElement : null,
  );

  if (!updater.detailsOpen || !updater.update) return null;

  const { update } = updater;
  const checking = updater.phase === "checking";
  const busy = updater.phase === "downloading" || updater.phase === "installing";
  const canInstall = updater.buildInfo?.canInstall ?? false;
  const rawNotes = updater.liveNotes ?? update.notes ?? "";
  const cleanedNotes = stripPlatformNotesSection(rawNotes);
  const errorKey = resolveUpdateErrorKey(updater.errorCode);
  const isSkipped = updater.skippedVersion === update.version;

  return (
    <SettingsDialog
      label={t("updates.availableTitle", { version: update.version })}
      saving={busy}
      returnFocusRef={capturedFocusRef}
      className="update-details-dialog"
      autoFocus
      onClose={updater.closeDetails}
    >
      <div className="api-profile-editor">
        <div className="api-profile-editor-heading update-details-heading">
          <div className="update-details-title-row">
            <span className={`update-channel-badge ${update.channel}`}>
              {t(`updates.channel.${update.channel}`)}
            </span>
            <strong>
              {t("updates.availableTitle", { version: update.version })}
            </strong>
          </div>
          <div className="update-details-meta">
            <span>
              {t("updates.versionTransition", {
                current: update.currentVersion,
                next: update.version,
              })}
            </span>
            <button
              className="update-inline-link"
              type="button"
              onClick={() => void openReleasePage(update.releaseUrl)}
            >
              <ExternalLink size={12} aria-hidden="true" />
              {t("updates.viewOnGitHub")}
            </button>
          </div>
        </div>

        <div className="api-profile-editor-content update-details-content">
          {cleanedNotes ? (
            <UpdateNotesMarkdown>{cleanedNotes}</UpdateNotesMarkdown>
          ) : updater.liveNotesLoading ? (
            <p className="update-details-empty">{t("updates.loadingNotes")}</p>
          ) : (
            <p className="update-details-empty">{t("updates.noNotes")}</p>
          )}

          <UpdateDownloadProgress updater={updater} />

          {canInstall && transcriptionRunning && !busy && (
            <p className="update-details-warning" role="status">
              {t("updates.stopTranscriptionFirst")}
            </p>
          )}

          {canInstall && !transcriptionRunning && !busy && (
            <p className="update-details-hint">{t("updates.restartNotice")}</p>
          )}

          {updater.phase === "error" && (
            <div className="update-details-error" role="alert">
              <span>{t(`updates.errors.${errorKey}`)}</span>
              <button
                className="update-inline-link"
                type="button"
                onClick={() => void openReleasePage(update.releaseUrl)}
              >
                <ExternalLink size={12} aria-hidden="true" />
                {t("updates.manualDownloadHint")}
              </button>
            </div>
          )}
        </div>

        <div className="api-profile-editor-actions">
          <div>
            {isSkipped ? (
              <span className="update-details-skipped-label">{t("updates.versionSkipped")}</span>
            ) : !busy ? (
              <button
                className="secondary-button"
                type="button"
                disabled={updater.preferenceSaving}
                onClick={() => void updater.skipCurrentUpdate()}
              >
                {t("updates.skipVersion")}
              </button>
            ) : null}
          </div>
          <div className="settings-inline-actions">
            {!busy && (
              <button className="secondary-button" type="button" onClick={updater.closeDetails}>
                {t("updates.later")}
              </button>
            )}
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
          </div>
        </div>
      </div>
    </SettingsDialog>
  );
}
