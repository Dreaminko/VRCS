import { Download, RefreshCw, ScanText, Trash2 } from "lucide-react";
import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { ConfirmDialog } from "../../app/components/ConfirmDialog";
import { formatBytes } from "../settings-derived";
import { useOcrModels } from "./useOcrModels";

export function OcrModelManagerPanel({ locale, disabled }: {
  locale: string;
  disabled: boolean;
}) {
  const { t } = useTranslation();
  const { models, busy, removing, error, download, remove } = useOcrModels();
  const [confirmDelete, setConfirmDelete] = useState(false);
  const deleteButtonRef = useRef<HTMLButtonElement>(null);
  const state = models?.state ?? "missing";
  const progress = models?.total_bytes
    ? Math.min(1, Math.max(0, models.downloaded_bytes / models.total_bytes))
    : 0;

  return <section className="model-section local-ocr-model" aria-labelledby="local-ocr-heading">
    <div className="section-heading">
      <div>
        <ScanText size={18} />
        <h3 id="local-ocr-heading">{t("settings.vrOcr.localModel")}</h3>
      </div>
    </div>
    <div className="model-list">
      <article className={`model-row model-status-${state}`}>
        <div className="model-row-body">
          <div className="model-row-title">
            <strong>PP-OCRv6 small</strong>
            {models && <span className="model-size">
              {formatBytes(models.total_bytes, locale)}
            </span>}
          </div>
          <p role="status">{models
            ? t(`settings.vrOcr.modelStates.${state}`)
            : t("common.loading")}</p>
          {state === "downloading" && <div className="model-progress-wrap">
            <div className="model-progress-track" role="progressbar"
              aria-label={t("settings.recognition.downloadProgress", { name: "PP-OCRv6" })}
              aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(progress * 100)}>
              <span style={{ width: `${Math.max(0.02, progress) * 100}%` }} />
            </div>
            <span>{Math.round(progress * 100)}%</span>
          </div>}
          {(error || models?.error) && <p className="model-error" role="alert">{error || models?.error}</p>}
        </div>
        <div className="model-row-action">
          {state === "ready"
            ? <button ref={deleteButtonRef} className="model-delete-button" type="button"
              disabled={disabled || busy || removing}
              aria-label={t("settings.recognition.deleteModel", { name: "PP-OCRv6 small" })}
              onClick={() => setConfirmDelete(true)}>
              <Trash2 size={16} />{t("common.delete")}
            </button>
            : state === "downloading"
              ? <span className="model-download-state"><RefreshCw size={15} />{t("common.downloading")}</span>
              : <button className="model-download-button" type="button" disabled={disabled || busy}
                onClick={() => void download()}>
                <Download size={16} />{t(error || models?.error || state === "error" ? "common.retry" : "common.download")}
              </button>}
        </div>
      </article>
    </div>
    {confirmDelete && <ConfirmDialog
      title={t("common.delete")}
      message={t("settings.recognition.confirmDelete", { name: "PP-OCRv6 small" })}
      confirmLabel={t("common.delete")}
      busy={removing}
      icon={<Trash2 size={16} />}
      returnFocusRef={deleteButtonRef}
      onClose={() => setConfirmDelete(false)}
      onConfirm={() => {
        void remove().finally(() => setConfirmDelete(false));
      }} />}
  </section>;
}
