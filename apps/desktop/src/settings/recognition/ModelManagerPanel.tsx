import { Download, FolderOpen, HardDrive, RefreshCw, Trash2 } from "lucide-react";
import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { NATIVE_APP } from "../../app/app-environment";
import type { QwenModelRecord } from "../../providers/types";
import { formatBytes } from "../settings-derived";
import type { SaveState } from "../settings-types";
import { ModelDeleteDialog } from "../components/ModelDeleteDialog";

export function ModelManagerPanel({
  locale,
  disabled,
  qwen,
  directoryText,
  saveState,
  onSetDirectoryText,
  onUpdateDirectory,
  onChooseDirectory,
}: {
  locale: string;
  disabled: boolean;
  qwen: {
    models: QwenModelRecord[];
    ready: boolean;
    message: string;
    selectedId: string | null;
    onLoad: () => Promise<void>;
    onDownload: (model: QwenModelRecord) => Promise<void>;
    onCancel: (model: QwenModelRecord) => Promise<void>;
    onRemove: (model: QwenModelRecord) => Promise<void>;
  };
  directoryText: string;
  saveState: SaveState;
  onSetDirectoryText: (value: string) => void;
  onUpdateDirectory: (value: string) => void;
  onChooseDirectory: () => Promise<void>;
}) {
  const { t } = useTranslation();
  const [pendingRemoval, setPendingRemoval] = useState<{
    name: string;
    remove: () => Promise<void>;
  } | null>(null);
  const [removing, setRemoving] = useState(false);
  const returnFocusRef = useRef<HTMLButtonElement>(null);
  const requestRemoval = (name: string, remove: () => Promise<void>, button: HTMLButtonElement) => {
    returnFocusRef.current = button;
    setPendingRemoval({ name, remove });
  };
  const confirmRemoval = async () => {
    if (!pendingRemoval || removing) return;
    setRemoving(true);
    try {
      await pendingRemoval.remove();
    } finally {
      setRemoving(false);
      setPendingRemoval(null);
    }
  };
  const downloadingCount = qwen.models.filter(
    (model) => model.status === "downloading" || model.status === "verifying",
  ).length;
  const installedCount = qwen.models.filter((model) => model.status === "installed").length;
  const ready = qwen.ready;
  return (
    <section className="model-section recognition-models" aria-labelledby="local-models-heading">
      <div className="section-heading">
        <div>
          <HardDrive size={18} />
          <h3 id="local-models-heading">{t("settings.recognition.localModels")}</h3>
          <span>
            {downloadingCount
              ? t("settings.recognition.downloadingCount", { count: downloadingCount })
              : ready
                ? t("settings.recognition.installedCount", { count: installedCount })
                : t("common.loading")}
          </span>
        </div>
        <button className="secondary-button" type="button" disabled={!qwen.ready} onClick={() => void qwen.onLoad()}><RefreshCw size={15} />{t("common.refresh")}</button>
      </div>

      <div className="model-directory-setting">
        <label htmlFor="model-directory">
          <span>{t("settings.recognition.modelDirectory")}</span>
        </label>
        <div>
          <input
            id="model-directory"
            type="text"
            value={directoryText}
            disabled={disabled || downloadingCount > 0 || saveState === "saving"}
            spellCheck={false}
            onChange={(event) => onSetDirectoryText(event.target.value)}
            onBlur={() => onUpdateDirectory(directoryText)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                event.currentTarget.blur();
              }
            }}
          />
          <button
            className="secondary-button"
            type="button"
            disabled={!NATIVE_APP || disabled || downloadingCount > 0 || saveState === "saving"}
            title={NATIVE_APP ? t("settings.recognition.chooseFolder") : t("settings.recognition.browserPathHint")}
            onClick={() => void onChooseDirectory()}
          >
            <FolderOpen size={16} />
            {t("settings.recognition.chooseFolder")}
          </button>
        </div>
      </div>

      {!ready && qwen.models.length === 0 ? (
        <div className="model-list-pending" role="status">
          <RefreshCw size={17} />
          <span>{t("settings.recognition.checkingLocalModels")}</span>
        </div>
      ) : (
        <div className="model-list">
          {qwen.models.map((model) => {
            const busy = model.status === "downloading" || model.status === "verifying";
            const selectedModel = model.id === qwen.selectedId;
            return <article className={`model-row model-status-${model.status}`} key={model.id}>
              <div className="model-row-body">
                <div className="model-row-title">
                  <strong>Qwen3-ASR 0.6B Q8_0</strong>
                  {selectedModel && <span className="model-active-chip">{t("settings.recognition.inUse")}</span>}
                  <span className="model-size">{formatBytes(model.total_bytes, locale)}</span>
                </div>
                {model.status !== "installed" && <p>{t(`settings.recognition.qwenStatus.${model.status}`)}</p>}
                <code>{model.repository}</code>
                {busy && <div className="model-progress-wrap">
                  <div className="model-progress-track" role="progressbar" aria-label={t("settings.recognition.downloadProgress", { name: model.id })} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(model.progress * 100)}>
                    <span style={{ transform: `scaleX(${Math.max(0.02, model.progress)})` }} />
                  </div>
                  <span>{Math.round(model.progress * 100)}%</span>
                </div>}
                {model.error && <p className="model-error" role="alert">{model.error}</p>}
              </div>
              <div className="model-row-action">
                {busy ? <button className="secondary-button" type="button" onClick={() => void qwen.onCancel(model)}>{t("common.cancel")}</button>
                  : model.status === "installed" || model.status === "corrupt" ? (
                    <button className="model-delete-button" type="button" disabled={disabled || saveState === "saving"} aria-label={t("settings.recognition.deleteModel", { name: model.id })} onClick={(event) => requestRemoval("Qwen3-ASR 0.6B Q8_0", () => qwen.onRemove(model), event.currentTarget)}><Trash2 size={16} />{t("common.delete")}</button>
                  )
                    : <button className="model-download-button" type="button" onClick={() => void qwen.onDownload(model)}><Download size={16} />{t(model.status === "error" ? "common.retry" : "common.download")}</button>}
              </div>
            </article>;
          })}
        </div>
      )}
      {qwen.message && <p className="model-manager-feedback" role="status">{qwen.message}</p>}
      {pendingRemoval && <ModelDeleteDialog
        name={pendingRemoval.name}
        removing={removing}
        returnFocusRef={returnFocusRef}
        onClose={() => setPendingRemoval(null)}
        onConfirm={confirmRemoval}
      />}
    </section>
  );
}
