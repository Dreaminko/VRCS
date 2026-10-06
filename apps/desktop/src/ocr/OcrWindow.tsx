import { useCallback, useEffect, useState } from "react";
import { Check, Copy, RefreshCw, TriangleAlert, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { closeDesktopOcr, getDesktopOcrStatus, listenDesktopOcrStatus, scanDesktopOcr } from "./api";
import { applyDesktopOcrStatus, desktopOcrCopyText, type DesktopOcrStatus } from "./status";
import { localizedLanguageName } from "../translation-languages";
import { contentLanguageTag } from "../app/ui-language";
import "./ocr-window.css";

export function OcrWindow() {
  const { t, i18n } = useTranslation();
  const [status, setStatus] = useState<DesktopOcrStatus | null>(null);
  const [error, setError] = useState("");
  const [copied, setCopied] = useState(false);
  const close = useCallback(() => {
    void closeDesktopOcr().catch(() => setError(t("ocrWindow.errors.failed")));
  }, [t]);
  useEffect(() => {
    let disposed = false;
    let unlisten: () => void = () => undefined;
    const load = async () => {
      const stop = await listenDesktopOcrStatus((next) => {
        if (!disposed) {
          setStatus((current) => applyDesktopOcrStatus(current, next));
          setCopied(false);
          setError("");
        }
      });
      if (disposed) { stop(); return; }
      unlisten = stop;
      const next = await getDesktopOcrStatus();
      if (!disposed) setStatus((current) => applyDesktopOcrStatus(current, next));
    };
    void load().catch(() => { if (!disposed) setError(t("ocrWindow.errors.failed")); });
    return () => { disposed = true; unlisten(); };
  }, [t]);
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => { if (event.key === "Escape") close(); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [close]);
  const scan = async () => {
    setError("");
    try { await scanDesktopOcr(); } catch { setError(t("ocrWindow.errors.failed")); }
  };
  const copy = async () => {
    try { await navigator.clipboard.writeText(desktopOcrCopyText(status?.blocks ?? [])); setCopied(true); }
    catch { setError(t("ocrWindow.copyFailed")); }
  };
  const processing = Boolean(status && ["capturing", "recognizing", "translating"].includes(status.state));
  const nativeError = status?.error ?? status?.shortcut_error;
  const state = status?.state ?? "idle";
  const stateLabel = status ? t(`ocrWindow.states.${state}`) : t("common.loading");
  return <main className="ocr-window">
    <header className="window-chrome ocr-window-header" data-tauri-drag-region>
      <h1 data-tauri-drag-region>{t("ocrWindow.title")}</h1>
      <div className="window-actions">
        <button className="window-close" type="button" aria-label={t("ocrWindow.close")} title={t("ocrWindow.close")} onClick={close}><X size={15} strokeWidth={1.8} /></button>
      </div>
    </header>
    <div className="ocr-window-content" aria-busy={!status || processing}>
      {(error || nativeError) && <p className="error-banner ocr-window-error" role="alert">
        <TriangleAlert size={15} aria-hidden="true" />
        <span>{error || t(`ocrWindow.errors.${nativeError?.replace("desktop_ocr.", "")}`, { defaultValue: t("ocrWindow.errors.failed") })}</span>
      </p>}
      {status?.timed_out && <p className="ocr-window-notice" role="status"><TriangleAlert size={15} aria-hidden="true" />{t("ocrWindow.timedOut")}</p>}
      {!status?.blocks.length && !error && !nativeError && !status?.timed_out && <div className="empty-state ocr-window-empty">
        <p role="status" aria-live="polite">{state === "disabled" ? t("ocrWindow.errors.disabled") : stateLabel}</p>
      </div>}
      {Boolean(status?.blocks.length) && <div className="ocr-result-list">{status?.blocks.map((block) => <article className="ocr-result-block" key={block.source.id}>
        <p className="ocr-result-source" aria-label={t("ocrWindow.source")}>{block.source.text}</p>
        {block.translations.map((translation) => <div className="ocr-result-target" key={translation.target_language}>
          {block.translations.length > 1 && <small>{localizedLanguageName(translation.target_language, i18n.resolvedLanguage ?? "en-US")}</small>}
          <p className={`ocr-result-translation${translation.text === null ? " ocr-result-placeholder" : ""}`}
            lang={contentLanguageTag(translation.target_language)}>
            {translation.text ?? t(processing && translation.error_code !== "translation.not_configured"
              ? "ocrWindow.translationPending" : "ocrWindow.translationFailed")}
          </p>
        </div>)}
      </article>)}</div>}
    </div>
    <footer className="ocr-window-toolbar">
      <span className="ocr-window-status" role="status" aria-live="polite">
        {copied ? t("ocrWindow.copied") : ""}
      </span>
      <div className="ocr-window-actions">
        <button className="dock-button" type="button" disabled={!status?.blocks.length}
          aria-label={t(copied ? "ocrWindow.copied" : "ocrWindow.copy")} data-tooltip={t(copied ? "ocrWindow.copied" : "ocrWindow.copy")}
          onClick={() => void copy()}>
          {copied ? <Check /> : <Copy />}
        </button>
        <button className="dock-button active" type="button" disabled={!status || status.state === "disabled"}
          aria-label={t("ocrWindow.rescan")} data-tooltip={t("ocrWindow.rescan")} onClick={() => void scan()}>
          <RefreshCw />
        </button>
      </div>
    </footer>
  </main>;
}
