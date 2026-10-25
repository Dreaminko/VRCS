import { useCallback, useEffect, useRef, useState } from "react";
import { Check, Copy, Image, LoaderCircle, Mic, RefreshCw, ScanEye, Send, Square, TriangleAlert, X } from "lucide-react";
import { Trans, useTranslation } from "react-i18next";
import {
  askDesktopVision, cancelDesktopVision, captureDesktopVision, closeDesktopVision,
  finishDesktopVisionRecording, getDesktopVisionImage, startDesktopVisionRecording,
} from "./api";
import { canAskDesktopVision, DESKTOP_VISION_QUESTION_LIMIT, desktopVisionBusy, desktopVisionSendKey } from "./status";
import { useDesktopVisionSession } from "./useDesktopVisionSession";
import { useAppTheme } from "../app/useAppTheme";
import { VisionAnswerMarkdown } from "./VisionAnswerMarkdown";

import "./vision-window.css";

export function VisionWindow() {
  const { t } = useTranslation();
  useAppTheme();
  const { status, draft, setDraft, statusError } = useDesktopVisionSession();
  const [commandError, setCommandError] = useState("");
  const [pending, setPending] = useState(false);
  const pendingRef = useRef(false);
  const commandRevision = useRef(0);
  const [copied, setCopied] = useState(false);
  const [preview, setPreview] = useState<{ imageId: number; url: string } | null>(null);
  const [previewError, setPreviewError] = useState<number | null>(null);
  const input = useRef<HTMLTextAreaElement>(null);
  const composing = useRef(false);
  const imageId = status?.image_id ?? null;
  const busy = desktopVisionBusy(status);
  const enabled = Boolean(status?.available && status.state !== "disabled");
  const editable = enabled && imageId !== null && !busy && !pending;
  const sendable = canAskDesktopVision(status, draft) && !pending;
  const questionLength = Array.from(draft.trim()).length;
  const questionTooLong = questionLength > DESKTOP_VISION_QUESTION_LIMIT;

  useEffect(() => {
    let disposed = false;
    let url: string | null = null;
    setPreview(null);
    setPreviewError(null);
    if (imageId !== null) {
      void getDesktopVisionImage(imageId).then((bytes) => {
        if (disposed) return;
        url = URL.createObjectURL(new Blob([new Uint8Array(bytes)], { type: "image/png" }));
        setPreview({ imageId, url });
      }).catch(() => { if (!disposed) setPreviewError(imageId); });
    }
    return () => {
      disposed = true;
      if (url) URL.revokeObjectURL(url);
    };
  }, [imageId]);

  useEffect(() => setCopied(false), [status?.answer, imageId]);

  useEffect(() => {
    if (editable && status?.state === "editing") input.current?.focus();
  }, [imageId, status?.state, editable]);

  const run = useCallback(async (action: (isCurrent: () => boolean) => Promise<void>, supersede = false) => {
    if (pendingRef.current && !supersede) return;
    const revision = ++commandRevision.current;
    pendingRef.current = true;
    setPending(true);
    setCommandError("");
    try { await action(() => revision === commandRevision.current); }
    catch (error) {
      if (revision === commandRevision.current) setCommandError(error instanceof Error ? error.message : String(error));
    } finally {
      if (revision === commandRevision.current) { pendingRef.current = false; setPending(false); }
    }
  }, []);
  const close = useCallback(() => {
    // Closing must cancel native work even while another command is settling.
    void run(closeDesktopVision, true);
  }, [run]);
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !event.isComposing && event.keyCode !== 229 && !composing.current) {
        event.preventDefault();
        close();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [close]);

  const send = () => {
    if (!sendable || imageId === null) return;
    void run(() => askDesktopVision(draft.trim(), imageId));
  };
  const reselect = () => void run(async (isCurrent) => {
    await cancelDesktopVision();
    if (isCurrent()) await captureDesktopVision();
  }, true);
  const copy = async () => {
    if (!status?.answer) return;
    try { await navigator.clipboard.writeText(status.answer); setCopied(true); }
    catch { setCommandError(t("visionWindow.copyFailed")); }
  };
  const nativeError = status?.error;
  const errorCode = commandError || nativeError;
  const shortcutError = status?.shortcut_error;

  return <main className="vision-window">
    <header className="window-chrome vision-window-header" data-tauri-drag-region>
      <h1 data-tauri-drag-region><ScanEye size={15} aria-hidden="true" />{t("visionWindow.title")}</h1>
      <div className="window-actions">
        <button className="window-close" type="button" aria-label={t("visionWindow.close")} data-tooltip={t("visionWindow.close")} onClick={close}><X size={15} strokeWidth={1.8} /></button>
      </div>
    </header>
    <div className="vision-window-content" aria-busy={!status || busy || pending}>

      {(statusError || errorCode) && <div className="error-banner vision-window-error" role="alert">
        <TriangleAlert size={15} aria-hidden="true" />
        <div>
          <span>{t(`visionWindow.errors.${errorCode?.replace("desktop_vision.", "") ?? "statusFailed"}`, { defaultValue: t("visionWindow.errors.failed") })}</span>
          {errorCode && <details><summary>{t("visionWindow.errorDetail")}</summary><p>{errorCode}{status?.error_detail ? `\n${status.error_detail}` : ""}</p></details>}
        </div>
      </div>}
      {shortcutError && <p className="vision-window-notice" role="alert" data-tooltip={shortcutError}>
        <TriangleAlert size={15} aria-hidden="true" />
        {t(`visionWindow.errors.${shortcutError.replace("desktop_vision.", "")}`, { defaultValue: t("visionWindow.errors.shortcutFailed") })}
      </p>}
      {imageId === null && <div className="empty-state vision-window-empty">
        <span className="vision-window-empty-icon" aria-hidden="true"><ScanEye size={32} strokeWidth={1.5} /></span>
        <h2>{t("visionWindow.title")}</h2>
        <p>{enabled
          ? <Trans i18nKey="visionWindow.captureHint" values={{ shortcut: status?.shortcut || "Ctrl+Alt+V" }} components={{ shortcut: <kbd /> }} />
          : t("visionWindow.errors.disabled")}</p>
      </div>}
      {imageId !== null && <>
        <figure className="settings-surface vision-window-image-card">
          <figcaption className="vision-window-section-heading"><Image size={15} aria-hidden="true" />{t("visionWindow.preview")}</figcaption>
          <div className="vision-window-preview">
            {previewError === imageId
              ? <p role="alert"><TriangleAlert size={18} aria-hidden="true" />{t("visionWindow.errors.previewFailed")}</p>
              : preview?.imageId === imageId
                ? <img src={preview.url} alt={t("visionWindow.preview")} onError={() => setPreviewError(imageId)} />
                : <p role="status"><LoaderCircle className="spinning" size={18} aria-hidden="true" />{t("visionWindow.loadingPreview")}</p>}
          </div>
        </figure>
        <div className="settings-surface vision-window-question-card">
          <label className="field cloud-text-field vision-window-question" htmlFor="vision-question">
            <span className="vision-window-question-heading">
              <span>{t("visionWindow.question")}</span>
              <span className="vision-window-question-count" aria-hidden="true">{questionLength} / {DESKTOP_VISION_QUESTION_LIMIT}</span>
            </span>
            <textarea id="vision-question" ref={input} value={draft} rows={3} disabled={!editable}
              aria-label={t("visionWindow.question")}
              aria-invalid={questionTooLong || undefined}
              aria-describedby={questionTooLong ? "vision-question-limit" : undefined}
              onChange={(event) => setDraft(event.target.value)}
              onCompositionStart={() => { composing.current = true; }} onCompositionEnd={() => { composing.current = false; }}
              onKeyDown={(event) => {
                if (!composing.current && desktopVisionSendKey(event.nativeEvent)) {
                  event.preventDefault();
                  send();
                }
              }} />

            {questionTooLong && <small id="vision-question-limit" role="status">{t("visionWindow.questionTooLong", { limit: DESKTOP_VISION_QUESTION_LIMIT })}</small>}
          </label>
          {status?.state === "recording" && <p className="vision-window-recording-draft" role="status"><Mic size={14} aria-hidden="true" />{t("visionWindow.recordingDraft", { question: status.question })}</p>}
        </div>

      </>}
      {status?.answer && <section className="vision-window-answer" aria-label={t("visionWindow.answer")}>
        <div className="bubble vision-window-answer-text">
          <VisionAnswerMarkdown>{status.answer}</VisionAnswerMarkdown>
        </div>
        <div className="vision-window-answer-actions">
          <button className="secondary-button vision-window-ask-again" type="button" disabled={!editable} onClick={() => { input.current?.focus(); input.current?.select(); }}>{t("visionWindow.askAgain")}</button>
          <button className="secondary-button vision-window-copy" type="button" aria-label={t(copied ? "visionWindow.copied" : "visionWindow.copy")} onClick={() => void copy()}>
            {copied ? <Check size={14} aria-hidden="true" /> : <Copy size={14} aria-hidden="true" />}
            {t(copied ? "visionWindow.copied" : "visionWindow.copy")}
          </button>
        </div>
      </section>}

    </div>
    <footer className="vision-window-toolbar">
      <span className="vision-window-copy-feedback" role="status" aria-live="polite">{copied ? t("visionWindow.copied") : ""}</span>
      <div className="vision-window-actions" role="group" aria-label={t("visionWindow.title")}>
        <button className="dock-button" type="button" disabled={!enabled} aria-label={t(imageId === null ? "visionWindow.capture" : "visionWindow.reselect")} data-tooltip={t(imageId === null ? "visionWindow.capture" : "visionWindow.reselect")} onClick={reselect}>
          {imageId === null ? <ScanEye aria-hidden="true" /> : <RefreshCw aria-hidden="true" />}

        </button>
        <button className="dock-button" type="button" disabled={!busy && !pending} aria-label={t("visionWindow.cancel")} data-tooltip={t("visionWindow.cancel")} onClick={() => void run(cancelDesktopVision, true)}><X /></button>
        <span className="dock-divider" aria-hidden="true" />
        <button className={`dock-button${status?.state === "recording" ? " active" : ""}`} type="button"
          aria-pressed={status?.state === "recording"}
          disabled={pending || (!editable && status?.state !== "recording")} aria-label={t(status?.state === "recording" ? "visionWindow.finishRecording" : "visionWindow.startRecording")}
          data-tooltip={t(status?.state === "recording" ? "visionWindow.finishRecording" : "visionWindow.startRecording")}
          onClick={() => void run(status?.state === "recording" ? finishDesktopVisionRecording : startDesktopVisionRecording)}>{status?.state === "recording" ? <Square /> : <Mic />}</button>
        <button className="dock-button primary" type="button" disabled={!sendable} aria-label={t("visionWindow.send")} data-tooltip={t("visionWindow.send")} onClick={send}>
          {status?.state === "requesting" ? <LoaderCircle className="spinning" aria-hidden="true" /> : <Send aria-hidden="true" />}
        </button>
      </div>
    </footer>
  </main>;
}
