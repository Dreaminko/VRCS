import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  ChevronDown,
  ChevronUp,
  Languages,
  LoaderCircle,
  Send,
} from "lucide-react";

import type {
  ChatboxComposeInput,
  ChatboxPreview,
} from "../types";
import { DropdownField } from "../shared/ui/DropdownField";
import { LanguagePicker } from "../shared/ui/LanguagePicker";

export function ChatboxComposer({
  draft,
  preview,
  busy,
  feedback,
  translationStale,
  oscEnabled,
  languageCodes,
  allowCustomLanguage,
  onDraftChange,
  onTranslate,
  onSend,
}: {
  draft: ChatboxComposeInput;
  preview: ChatboxPreview;
  busy: "translate" | "send" | null;
  feedback: { tone: "success" | "error"; text: string } | null;
  translationStale: boolean;
  oscEnabled: boolean;
  languageCodes: readonly string[];
  allowCustomLanguage: boolean;
  onDraftChange: (draft: ChatboxComposeInput) => void;
  onTranslate: () => Promise<void>;
  onSend: () => Promise<boolean>;
}) {
  const { t } = useTranslation();
  const originalRef = useRef<HTMLTextAreaElement | null>(null);
  const [expanded, setExpanded] = useState(false);

  useEffect(() => {
    const frame = requestAnimationFrame(() => originalRef.current?.focus());
    return () => cancelAnimationFrame(frame);
  }, []);

  const update = (partial: Partial<ChatboxComposeInput>) => {
    onDraftChange({ ...draft, ...partial });
  };
  const translationAvailable = Boolean(draft.translation?.trim());
  const translationPending = draft.send_mode !== "original"
    && (!translationAvailable || translationStale);
  const canSend = oscEnabled
    && busy === null
    && Boolean(draft.original.trim())
    && (translationPending || preview.sendable);
  const inputCount = [...draft.original].length;
  const sendLabel = busy === "translate"
    ? t("translation.translating")
    : busy === "send"
      ? t("chatbox.sending")
      : t("chatbox.send");
  const submit = () => {
    if (!canSend) return;
    void onSend().then((sent) => {
      if (sent) originalRef.current?.focus();
    });
  };

  return (
    <section
      className={`chatbox-composer ${expanded ? "expanded" : "compact"}`}
      aria-label={t("chatbox.title")}
      aria-busy={busy === "translate" || busy === "send"}
      onKeyDown={(event) => {
        if (event.key === "Escape" && expanded) {
          event.stopPropagation();
          setExpanded(false);
          originalRef.current?.focus();
          return;
        }
        if (event.key === "Enter" && event.ctrlKey && canSend) {
          event.preventDefault();
          submit();
        }
      }}
    >
      {expanded && (
        <div className="chatbox-details" id="chatbox-details">
          <header className="chatbox-details-header">
            <div>
              <span className="chatbox-eyebrow">VRChat</span>
              <h2>{t("chatbox.detailsTitle")}</h2>
            </div>
            <span>{t("chatbox.translationOnSend")}</span>
          </header>

          <div className="chatbox-controls">
            <div className="chatbox-language-control">
              <LanguagePicker
                compact
                floating
                label={t("chatbox.targetLanguage")}
                value={draft.target_language ?? "ja"}
                languageCodes={languageCodes}
                allowCustom={allowCustomLanguage}
                onChange={(target_language) => update({ target_language })}
              />
              <button
                className="chatbox-secondary-button"
                type="button"
                disabled={busy !== null || !draft.original.trim()}
                onClick={() => void onTranslate()}
              >
                {busy === "translate" ? <LoaderCircle className="chatbox-spinner" size={16} /> : <Languages size={16} />}
                {t(busy === "translate" ? "translation.translating" : "chatbox.translate")}
              </button>
            </div>
            <SegmentedControl
              label={t("chatbox.sendMode")}
              value={draft.send_mode}
              options={[
                { value: "original", label: t("chatbox.modes.original") },
                { value: "translation", label: t("chatbox.modes.translation") },
                { value: "bilingual", label: t("chatbox.modes.bilingual") },
              ]}
              onChange={(send_mode) => update({ send_mode })}
            />
          </div>

          <label className={`chatbox-text-field ${translationStale ? "stale" : ""}`}>
            <span>
              {t("chatbox.translation")}
              {translationStale && <small>{t("chatbox.translationStale")}</small>}
            </span>
            <textarea
              value={draft.translation ?? ""}
              maxLength={5000}
              placeholder={t("chatbox.translationPlaceholder")}
              disabled={busy === "translate" || busy === "send"}
              onChange={(event) => update({ translation: event.target.value || null })}
            />
          </label>

          {draft.send_mode === "bilingual" && (
            <div className={`chatbox-format-row ${draft.message_format === "custom" ? "custom" : ""}`}>
              <DropdownField
                compact
                floating
                label={t("chatbox.format")}
                value={draft.message_format}
                options={[
                  { value: "original_newline_translation", label: t("chatbox.formats.originalFirst") },
                  { value: "translation_newline_original", label: t("chatbox.formats.translationFirst") },
                  { value: "slash_separated", label: t("chatbox.formats.slash") },
                  { value: "custom", label: t("chatbox.formats.custom") },
                ]}
                onChange={(message_format) => update({ message_format: message_format as ChatboxComposeInput["message_format"] })}
              />
              {draft.message_format === "custom" && (
                <input
                  className="chatbox-custom-format"
                  aria-label={t("chatbox.customFormat")}
                  value={draft.custom_format ?? ""}
                  maxLength={200}
                  placeholder="{original} / {translation}"
                  onChange={(event) => update({ custom_format: event.target.value || null })}
                />
              )}
            </div>
          )}

          <div className={`chatbox-preview ${!translationPending && preview.over_limit ? "over-limit" : ""}`}>
            <div className="chatbox-preview-heading">
              <span>{t("chatbox.preview")}</span>
              <output>{translationPending ? `— / ${preview.limit}` : `${preview.char_count} / ${preview.limit}`}</output>
            </div>
            <p>{translationPending
              ? t("chatbox.translationPreviewPending")
              : preview.text || t("chatbox.previewEmpty")}</p>
            <label className="chatbox-overflow-choice">
              <input
                type="checkbox"
                checked={draft.overflow_policy === "smart_truncate"}
                onChange={(event) => update({
                  overflow_policy: event.target.checked ? "smart_truncate" : "block",
                })}
              />
              <span>{t("chatbox.smartTruncate")}</span>
            </label>
          </div>
        </div>
      )}

      <div className="chatbox-quick-row">
        <button
          className="chatbox-expand-button"
          type="button"
          aria-expanded={expanded}
          aria-controls="chatbox-details"
          aria-label={t(expanded ? "chatbox.collapseDetails" : "chatbox.expandDetails")}
          title={t(expanded ? "chatbox.collapseDetails" : "chatbox.expandDetails")}
          onClick={() => setExpanded((current) => !current)}
        >
          {expanded ? <ChevronDown size={20} /> : <ChevronUp size={20} />}
        </button>
        <div className={`chatbox-quick-input ${inputCount > preview.limit ? "over-limit" : ""}`}>
          <label className="sr-only" htmlFor="chatbox-original-input">{t("chatbox.original")}</label>
          <textarea
            id="chatbox-original-input"
            ref={originalRef}
            rows={1}
            value={draft.original}
            maxLength={5000}
            placeholder={t("chatbox.originalPlaceholder")}
            disabled={busy === "translate" || busy === "send"}
            onChange={(event) => update({ original: event.target.value })}
            onKeyDown={(event) => {
              if (event.key !== "Enter" || event.shiftKey || event.nativeEvent.isComposing) return;
              event.preventDefault();
              event.stopPropagation();
              submit();
            }}
          />
          <output aria-label={t("chatbox.inputCount", { count: inputCount })}>{inputCount}</output>
        </div>
        <button
          className="chatbox-send-button"
          type="button"
          disabled={!canSend}
          aria-label={sendLabel}
          title={sendLabel}
          onClick={submit}
        >
          {busy === "translate" || busy === "send"
            ? <LoaderCircle className="chatbox-spinner" size={20} />
            : <Send size={20} />}
        </button>
      </div>

      {!oscEnabled && <p className="chatbox-inline-message error">{t("chatbox.oscDisabled")}</p>}
      {feedback && <p className={`chatbox-inline-message ${feedback.tone}`} role="status">{feedback.text}</p>}
    </section>
  );
}

function SegmentedControl<T extends string>({ label, value, options, onChange }: {
  label: string;
  value: T;
  options: Array<{ value: T; label: string }>;
  onChange: (value: T) => void;
}) {
  return (
    <div className="chatbox-segmented" role="group" aria-label={label}>
      {options.map((option) => (
        <button
          type="button"
          key={option.value}
          className={option.value === value ? "active" : ""}
          aria-pressed={option.value === value}
          onClick={() => onChange(option.value)}
        >{option.label}</button>
      ))}
    </div>
  );
}
