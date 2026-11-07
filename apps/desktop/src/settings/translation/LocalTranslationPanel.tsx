import { Download, HardDrive, RefreshCw, Square, Trash2 } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { localizedError } from "../../app/app-utils";
import { ConfirmDialog } from "../../app/components/ConfirmDialog";
import { providersApi } from "../../providers/api";
import { localTranslationApi } from "../local-translation-api";
import { formatBytes } from "../settings-derived";
import type { LocalTranslationStatus } from "../types";

export function LocalTranslationPanel({
  locale,
  disabled,
}: {
  locale: string;
  disabled: boolean;
}) {
  const { t } = useTranslation();
  const [status, setStatus] = useState<LocalTranslationStatus | null>(null);
  const [statusError, setStatusError] = useState("");
  const [actionMessage, setActionMessage] = useState("");
  const [busy, setBusy] = useState(false);
  const [accepted, setAccepted] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const deleteButton = useRef<HTMLButtonElement>(null);

  const load = useCallback(async () => {
    try {
      const next = await localTranslationApi.status();
      setStatus(next);
      setStatusError("");
    } catch (reason) {
      setStatusError(localizedError(reason, t, "errors.apiStatus.translation.failed"));
    }
  }, [t]);

  useEffect(() => {
    let cancelled = false;
    let timer: number | null = null;
    const poll = async () => {
      if (!cancelled) await load();
      if (!cancelled) timer = window.setTimeout(() => void poll(), 1_000);
    };
    void poll();
    return () => {
      cancelled = true;
      if (timer !== null) window.clearTimeout(timer);
    };
  }, [load]);

  useEffect(() => setAccepted(status?.model.license_accepted ?? false), [status?.model.license_accepted]);

  const run = async (action: () => Promise<unknown>) => {
    if (busy) return;
    setBusy(true);
    setActionMessage("");
    try {
      await action();
      await load();
    } catch (reason) {
      setActionMessage(localizedError(reason, t, "errors.apiStatus.translation.failed"));
    } finally {
      setBusy(false);
    }
  };

  const model = status?.model;
  const runtime = status?.runtime;
  const license = status?.license;
  const downloading = model?.status === "downloading" || model?.status === "verifying";
  const needsRuntime = Boolean(runtime && (!runtime.runtime_available || runtime.error));
  return <section className="model-section local-translation-model" aria-labelledby="local-translation-heading">
    <div className="section-heading">
      <div>
        <HardDrive size={18} />
        <h3 id="local-translation-heading">{t("settings.translation.localTranslation")}</h3>
        <span>{model
          ? t(`settings.translation.localStatus.${model.status}`)
          : t("common.loading")}</span>
      </div>
      <button className="secondary-button" type="button" disabled={disabled || busy}
        onClick={() => void load()}>
        <RefreshCw size={15} />{t("common.refresh")}
      </button>
    </div>

    {!model || !runtime || !license ? <div className="model-list-pending" role="status">
      <RefreshCw size={17} /><span>{t("settings.translation.checkingLocalModel")}</span>
    </div>
      : <div className="model-list">
        {needsRuntime && <article className={`model-row model-status-${runtime.status}`}>
          <div className="model-row-body">
            <div className="model-row-title">
              <strong>{t("settings.translation.localRuntimeTitle")}</strong>
              <span className="model-size">{runtime.device.toUpperCase()}</span>
            </div>
            <p role="status">{runtime.runtime_available
              ? t(`settings.translation.localStatus.${runtime.status}`)
              : t("settings.translation.localRuntimeMissing")}</p>
            {runtime.error && <p className="model-error" role="alert">{localizedError({
              code: "translation.local_runtime_unavailable",
              detail: runtime.error,
            }, t, "errors.apiStatus.translation.failed")}</p>}
          </div>
          <div className="model-row-action">
            {!runtime.runtime_available
              && <button className="model-download-button" type="button" disabled={disabled || busy}
                onClick={() => void run(providersApi.downloadQwenRuntime)}>
                <Download size={16} />{t("settings.translation.localDownloadRuntime")}
              </button>}
          </div>
        </article>}
        <article className={`model-row model-status-${model.status}`}>
          <div className="model-row-body">
            <div className="model-row-title">
              <strong>PLaMo 3 Translate 0.8B Q4_K_M</strong>
              {runtime.status === "ready" && <span className="model-active-chip">
                {t("settings.recognition.inUse")}
              </span>}
              <span className="model-size">{formatBytes(model!.total_bytes, locale)}</span>
            </div>
            {model.status !== "installed" && <p>{t(`settings.translation.localStatus.${model.status}`)}</p>}
            <code>{model.id}@{model.revision}</code>
            {downloading && <div className="model-progress-wrap">
              <div className="model-progress-track" role="progressbar"
                aria-label={t("settings.translation.localDownloadProgress")}
                aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(model.progress * 100)}>
                <span style={{ transform: `scaleX(${Math.max(0.02, model.progress)})` }} />
              </div>
              <span>{Math.round(model.progress * 100)}%</span>
            </div>}
            {model.error && <p className="model-error" role="alert">{localizedError({
              code: "translation.local_download_failed",
              detail: model.error,
            }, t, "errors.apiStatus.translation.failed")}</p>}
          </div>
          <div className="model-row-action">
            {downloading
              ? <button className="secondary-button" type="button" disabled={disabled || busy}
                onClick={() => void run(localTranslationApi.cancel)}>
                <Square size={14} />{t("common.cancel")}
              </button>
              : model.status === "installed" ?
                <button ref={deleteButton} className="model-delete-button" type="button" disabled={disabled || busy}
                  aria-label={t("settings.translation.deleteLocalModel")} onClick={() => setConfirmDelete(true)}>
                  <Trash2 size={16} />{t("common.delete")}
                </button>
              : <button className="model-download-button" type="button" disabled={disabled || busy || !accepted}
                onClick={() => void run(() => localTranslationApi.download(model.id, model.revision))}>
                <Download size={16} />{t(model.status === "error" ? "common.retry" : "common.download")}
              </button>}
          </div>
        </article>
        {model.status !== "installed" && <label className="local-license-consent">
          <input type="checkbox" checked={accepted} disabled={disabled || downloading}
            onChange={(event) => setAccepted(event.target.checked)} />
          <span>{t("settings.translation.localLicenseConsent")} </span>
          <a href={license.url} target="_blank" rel="noreferrer">{license.name}</a>
        </label>}
      </div>}
    {license && <p className="local-model-license-note">
      {license.attribution} · {t("settings.translation.localLicenseNotice")}
    </p>}
    {(statusError || actionMessage) && <p className="model-manager-feedback" role="alert">
      {statusError || actionMessage}
    </p>}
    {confirmDelete && <ConfirmDialog title={t("settings.translation.deleteLocalModel")}
      message={t("settings.translation.confirmDeleteLocalModel")} confirmLabel={t("common.delete")}
      busy={busy} returnFocusRef={deleteButton} onClose={() => setConfirmDelete(false)}
      onConfirm={() => {
        setConfirmDelete(false);
        if (model) void run(() => localTranslationApi.delete(model.id));
      }} />}
  </section>;
}
