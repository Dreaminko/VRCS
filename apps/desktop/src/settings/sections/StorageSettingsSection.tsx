import { useCallback, useEffect, useState } from "react";
import { Database, HardDrive, RefreshCw, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";

import { coreApi } from "../../api";
import { localizedError } from "../../app-utils";
import type { DatabaseStorageStats, Settings } from "../../types";
import { formatBytes } from "../settings-derived";
import type { ApplySettings, SaveState } from "../settings-types";

const MEBIBYTE = 1024 * 1024;
const MIN_QUOTA_MIB = 10;
const MAX_QUOTA_MIB = 10 * 1024;

export function StorageSettingsSection({
  locale,
  draft,
  saveState,
  applySettings,
}: {
  locale: string;
  draft: Settings;
  saveState: SaveState;
  applySettings: ApplySettings;
}) {
  const { t } = useTranslation();
  const [stats, setStats] = useState<DatabaseStorageStats | null>(null);
  const [quotaText, setQuotaText] = useState(() => String(
    Math.round(draft.storage.subtitle_history_max_bytes / MEBIBYTE),
  ));
  const [busy, setBusy] = useState<"refresh" | "clear" | null>(null);
  const [message, setMessage] = useState("");

  useEffect(() => {
    setQuotaText(String(Math.round(draft.storage.subtitle_history_max_bytes / MEBIBYTE)));
  }, [draft.storage.subtitle_history_max_bytes]);

  const loadStats = useCallback(async (showBusy = false) => {
    if (showBusy) setBusy("refresh");
    try {
      setStats(await coreApi.storageStats());
      setMessage("");
    } catch (reason) {
      setMessage(localizedError(reason, t, "errors.storage.stats_failed"));
    } finally {
      if (showBusy) setBusy(null);
    }
  }, [t]);

  useEffect(() => {
    void loadStats();
    const timer = window.setInterval(() => void loadStats(), 5_000);
    return () => window.clearInterval(timer);
  }, [loadStats]);

  const commitQuota = () => {
    const value = Number.parseInt(quotaText, 10);
    if (!Number.isInteger(value) || value < MIN_QUOTA_MIB || value > MAX_QUOTA_MIB) {
      setMessage(t("settings.storage.quotaInvalid", {
        min: MIN_QUOTA_MIB,
        max: new Intl.NumberFormat(locale).format(MAX_QUOTA_MIB),
      }));
      return;
    }
    const maxBytes = value * MEBIBYTE;
    if (maxBytes === draft.storage.subtitle_history_max_bytes) return;
    applySettings(
      (current) => ({
        ...current,
        storage: { ...current.storage, subtitle_history_max_bytes: maxBytes },
      }),
      () => {
        void loadStats();
        window.dispatchEvent(new Event("vrcs:subtitle-history-refresh"));
      },
      () => setQuotaText(String(Math.round(draft.storage.subtitle_history_max_bytes / MEBIBYTE))),
    );
  };

  const clearHistory = async () => {
    if (!window.confirm(t("settings.storage.clearConfirm"))) return;
    setBusy("clear");
    setMessage("");
    try {
      setStats(await coreApi.clearSubtitleHistory());
      setMessage(t("settings.storage.clearComplete"));
      window.dispatchEvent(new Event("vrcs:subtitle-history-refresh"));
    } catch (reason) {
      setMessage(localizedError(reason, t, "errors.storage.clear_failed"));
    } finally {
      setBusy(null);
    }
  };

  const usagePercent = stats && stats.max_bytes > 0
    ? Math.min(100, (stats.used_bytes / stats.max_bytes) * 100)
    : 0;

  return (
    <section className="system-settings-group storage-section" aria-labelledby="system-storage-title">
      <div className="section-heading">
        <div><HardDrive size={18} /><h3 id="system-storage-title">{t("settings.storage.title")}</h3></div>
      </div>

      <div className="storage-usage-card">
        <div className="storage-usage-heading">
          <span className="storage-heading-icon" aria-hidden="true"><Database size={18} /></span>
          <div>
            <strong>{t("settings.storage.databaseUsage")}</strong>
          </div>
          <button className="secondary-button storage-refresh-button" type="button" disabled={busy !== null} onClick={() => void loadStats(true)}>
            <RefreshCw size={15} className={busy === "refresh" ? "spinning" : ""} />
            {t("common.refresh")}
          </button>
        </div>
        <div className="storage-meter" role="progressbar" aria-label={t("settings.storage.databaseUsage")} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(usagePercent)}>
          <span style={{ transform: `scaleX(${usagePercent / 100})` }} />
        </div>
        <div className="storage-usage-summary">
          <strong>{stats ? formatBytes(stats.used_bytes, locale) : t("common.loading")}</strong>
          <span>{stats ? t("settings.storage.ofQuota", { quota: formatBytes(stats.max_bytes, locale) }) : ""}</span>
        </div>
        {stats && (
          <div className="storage-stat-grid">
            <div><span>{t("settings.storage.allocated")}</span><strong>{formatBytes(stats.allocated_bytes, locale)}</strong></div>
            <div><span>{t("settings.storage.reclaimable")}</span><strong>{formatBytes(stats.reclaimable_bytes, locale)}</strong></div>
          </div>
        )}
        {stats?.over_limit && <p className="storage-over-limit">{t("settings.storage.overLimit")}</p>}
      </div>

      <div className="storage-setting-row">
        <div>
          <strong>{t("settings.storage.quota")}</strong>
        </div>
        <label className="storage-quota-input">
          <span>{t("settings.storage.quota")}</span>
          <span>
            <input
              type="number"
              min={MIN_QUOTA_MIB}
              max={MAX_QUOTA_MIB}
              step={10}
              value={quotaText}
              disabled={saveState === "saving"}
              onChange={(event) => setQuotaText(event.target.value)}
              onBlur={commitQuota}
              onKeyDown={(event) => {
                if (event.key === "Enter") event.currentTarget.blur();
              }}
            />
            <em>MiB</em>
          </span>
        </label>
      </div>

      <div className="storage-clear-row">
        <div>
          <strong>{t("settings.storage.clearHistory")}</strong>
        </div>
        <button className="secondary-button storage-clear-button" type="button" disabled={busy !== null} onClick={() => void clearHistory()}>
          <Trash2 size={15} />
          {busy === "clear" ? t("settings.storage.clearing") : t("settings.storage.clearAction")}
        </button>
      </div>

      {message && <p className="storage-feedback" role="status">{message}</p>}
    </section>
  );
}
