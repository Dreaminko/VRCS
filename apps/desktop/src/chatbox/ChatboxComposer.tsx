import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  ChevronUp,
  Languages,
  LoaderCircle,
  Send,
} from "lucide-react";

import type {
  ChatboxPreview,
} from "./types";
import {
  type ChatboxDraft,
  updateChatboxTranslation,
} from "./chatbox";
import { LanguagePicker } from "../shared/ui/LanguagePicker";
import type { DropdownOption } from "../shared/ui/DropdownField";
import {
  languageSearchText,
  localizedLanguageName,
  translationLanguage,
} from "../translation/translation-languages";

const FOLLOW_TRANSLATION_VALUE = "__follow_translation__";

export function ChatboxComposer({
  draft,
  preferredTarget,
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
  inline = false,
  closing = false,
  expanded: controlledExpanded,
  onExpandedChange,
  onClose,
  onCloseComplete,
}: {
  draft: ChatboxDraft;
  preferredTarget: string;
  preview: ChatboxPreview;
  busy: "translate" | "send" | null;
  feedback: { tone: "success" | "error"; text: string } | null;
  translationStale: boolean;
  oscEnabled: boolean;
  languageCodes: readonly string[];
  allowCustomLanguage: boolean;
  onDraftChange: (draft: ChatboxDraft) => void;
  onTranslate: () => Promise<void>;
  onSend: () => Promise<boolean>;
  inline?: boolean;
  closing?: boolean;
  expanded?: boolean;
  onExpandedChange?: (expanded: boolean) => void;
  onClose?: () => void;
  onCloseComplete?: () => void;
}) {
  const { t, i18n } = useTranslation();
  const originalRef = useRef<HTMLTextAreaElement | null>(null);
  const [localExpanded, setLocalExpanded] = useState(false);
  const expanded = controlledExpanded ?? localExpanded;
  const setExpanded = (next: boolean) => {
    setLocalExpanded(next);
    onExpandedChange?.(next);
  };

  useEffect(() => {
    if (closing) return;
    const frame = requestAnimationFrame(() => originalRef.current?.focus());
    return () => cancelAnimationFrame(frame);
  }, [closing]);

  const update = (partial: Partial<ChatboxDraft>) => {
    onDraftChange({ ...draft, ...partial });
  };
  const controlsDisabled = busy !== null;
  const usesTranslation = draft.send_mode !== "original";
  const effectiveTarget = draft.follow_translation_settings ? preferredTarget : draft.target_language;
  const locale = i18n.resolvedLanguage ?? "en-US";
  const followLanguageOptions = useMemo<DropdownOption[]>(() => {
    const preferredLanguage = translationLanguage(preferredTarget);
    const followLabel = t("chatbox.followTranslationSettings");
    return [{
      value: FOLLOW_TRANSLATION_VALUE,
      label: `${followLabel} · ${localizedLanguageName(preferredTarget, locale)}`,
      description: preferredTarget,
      searchText: `${followLabel} ${preferredLanguage ? languageSearchText(preferredLanguage, locale) : preferredTarget}`,
    }];
  }, [locale, preferredTarget, t]);
  const modeSummary = usesTranslation
    ? `${t(`chatbox.modes.${draft.send_mode}`)} · ${localizedLanguageName(effectiveTarget, locale)}`
    : t("chatbox.modes.original");
  const translationAvailable = Boolean(draft.translation?.trim());
  const translationPending = usesTranslation
    && (!translationAvailable || translationStale);
  const previewBlocked = !translationPending && preview.over_limit && !preview.sendable;
  const previewTruncated = !translationPending && preview.truncated;
  const sendDisabledReason = !oscEnabled
    ? t("chatbox.oscDisabled")
    : previewBlocked
      ? t("chatbox.overLimitHint", { limit: preview.limit })
      : null;
  const canSend = oscEnabled
    && !controlsDisabled
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

  const detailsPanel = (
    <div className="chatbox-details-reveal" aria-hidden={!expanded} inert={!expanded}>
      <div className="chatbox-details-clip">
        <div className="chatbox-details" id="chatbox-details">
          <header className="chatbox-details-header">
            <h2>{t("chatbox.detailsTitle")}</h2>
            <SegmentedControl
              label={t("chatbox.sendMode")}
              value={draft.send_mode}
              disabled={controlsDisabled}
              options={[
                { value: "original", label: t("chatbox.modes.original") },
                { value: "translation", label: t("chatbox.modes.translation") },
                { value: "bilingual", label: t("chatbox.modes.bilingual") },
              ]}
              onChange={(send_mode) => update({ send_mode })}
            />
          </header>

          {usesTranslation && (
            <>
              <div className="chatbox-language-control">
                <LanguagePicker
                  compact
                  floating
                  label={t("chatbox.targetLanguage")}
                  value={draft.follow_translation_settings ? FOLLOW_TRANSLATION_VALUE : draft.target_language}
                  disabled={controlsDisabled}
                  languageCodes={languageCodes}
                  leadingOptions={followLanguageOptions}
                  allowCustom={allowCustomLanguage}
                  onChange={(next) => (next === FOLLOW_TRANSLATION_VALUE
                    ? update({ follow_translation_settings: true })
                    : update({ follow_translation_settings: false, target_language: next }))}
                />
                <button
                  className="chatbox-secondary-button"
                  type="button"
                  disabled={controlsDisabled || !draft.original.trim()}
                  onClick={() => void onTranslate()}
                >
                  {busy === "translate" ? <LoaderCircle className="chatbox-spinner" size={16} /> : <Languages size={16} />}
                  {t(busy === "translate" ? "translation.translating" : "chatbox.translate")}
                </button>
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
                  disabled={controlsDisabled}
                  onChange={(event) => onDraftChange(updateChatboxTranslation(draft, event.target.value))}
                />
              </label>
            </>
          )}

          <div className={`chatbox-preview ${previewBlocked ? "over-limit" : ""} ${previewTruncated ? "truncated" : ""}`}>
            <div className="chatbox-preview-heading">
              <span>{t("chatbox.preview")}</span>
              <output>
                {translationPending
                  ? `— / ${preview.limit}`
                  : previewTruncated
                    ? `${t("chatbox.truncatedBadge")} · ${preview.char_count} / ${preview.limit}`
                    : `${preview.char_count} / ${preview.limit}`}
              </output>
            </div>
            <p className={translationPending || !preview.text ? "is-placeholder" : undefined}>{translationPending
              ? t("chatbox.translationPreviewPending")
              : preview.text || t("chatbox.previewEmpty")}</p>
            <label className="chatbox-checkbox">
              <input
                type="checkbox"
                checked={draft.overflow_policy === "smart_truncate"}
                disabled={controlsDisabled}
                onChange={(event) => update({
                  overflow_policy: event.target.checked ? "smart_truncate" : "block",
                })}
              />
              <span>{t("chatbox.smartTruncate")}</span>
            </label>
          </div>
        </div>
      </div>
    </div>
  );

  return (
    <section
      className={`chatbox-composer ${expanded ? "expanded" : "compact"}${inline ? " chatbox-composer-inline" : ""}${closing ? " closing" : ""}`}
      aria-label={t("chatbox.title")}
      aria-hidden={closing || undefined}
      aria-busy={busy === "translate" || busy === "send"}
      inert={closing}
      onAnimationEnd={(event) => {
        if (closing && event.target === event.currentTarget && event.animationName === "chatbox-composer-out") {
          onCloseComplete?.();
        }
      }}
      onKeyDown={(event) => {
        if (event.nativeEvent.isComposing || event.nativeEvent.keyCode === 229) return;
        if (event.key === "Escape" && (expanded || onClose)) {
          event.preventDefault();
          event.stopPropagation();
          if (expanded) {
            setExpanded(false);
            originalRef.current?.focus();
          } else onClose?.();
          return;
        }
        if (event.key === "Enter" && event.ctrlKey && !event.shiftKey && canSend) {
          event.preventDefault();
          submit();
        }
      }}
    >
      {!inline && detailsPanel}

      <div className="chatbox-quick-row">
        <button
          className="chatbox-expand-button"
          type="button"
          aria-expanded={expanded}
          aria-controls="chatbox-details"
          aria-label={t(expanded ? "chatbox.collapseDetails" : "chatbox.expandDetails")}
          data-tooltip={t(expanded ? "chatbox.collapseDetails" : "chatbox.expandDetails")}
          onClick={() => setExpanded(!expanded)}
        >
          <ChevronUp
            size={20}
            className={`chatbox-expand-chevron ${expanded !== inline ? "rotated" : ""}`}
          />
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
            disabled={controlsDisabled}
            onChange={(event) => update({ original: event.target.value })}
            onKeyDown={(event) => {
              if (event.key !== "Enter" || event.shiftKey || event.nativeEvent.isComposing || event.nativeEvent.keyCode === 229) return;
              event.preventDefault();
              event.stopPropagation();
              submit();
            }}
          />
          <div className="chatbox-quick-meta">
            <button
              type="button"
              className="chatbox-mode-badge"
              aria-label={t(expanded ? "chatbox.collapseDetails" : "chatbox.expandDetails")}
              onClick={() => setExpanded(!expanded)}
            >
              {modeSummary}
            </button>
            <output aria-label={t("chatbox.inputCount", { count: inputCount })}>
              {inputCount > preview.limit ? `${inputCount}/${preview.limit}` : inputCount}
            </output>
          </div>
        </div>
        <button
          className="chatbox-send-button"
          type="button"
          disabled={!canSend}
          aria-label={sendLabel}
          data-tooltip={!controlsDisabled && sendDisabledReason ? sendDisabledReason : sendLabel}
          onClick={submit}
        >
          {controlsDisabled
            ? <LoaderCircle className="chatbox-spinner" size={20} />
            : <Send size={20} />}
        </button>
      </div>

      {!oscEnabled && <p className="chatbox-inline-message error">{t("chatbox.oscDisabled")}</p>}
      {feedback && <p className={`chatbox-inline-message ${feedback.tone}`} role="status">{feedback.text}</p>}
      {oscEnabled && !feedback && !expanded && Boolean(draft.original.trim()) && sendDisabledReason && (
        <p className="chatbox-inline-message error" role="status">{sendDisabledReason}</p>
      )}
      {inline && detailsPanel}
    </section>
  );
}

function SegmentedControl<T extends string>({ label, value, disabled = false, options, onChange }: {
  label: string;
  value: T;
  disabled?: boolean;
  options: Array<{ value: T; label: string }>;
  onChange: (value: T) => void;
}) {
  return (
    <div className="theme-toggle-group chatbox-segmented" role="group" aria-label={label}>
      {options.map((option) => (
        <button
          type="button"
          key={option.value}
          disabled={disabled}
          className={option.value === value ? "active" : ""}
          aria-pressed={option.value === value}
          onClick={() => onChange(option.value)}
        >{option.label}</button>
      ))}
    </div>
  );
}
