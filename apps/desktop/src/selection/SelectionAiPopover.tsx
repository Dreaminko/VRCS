import { useEffect, useState } from "react";
import {
  ArrowLeft,
  Check,
  Copy,
  LoaderCircle,
  RotateCcw,
  Send,
  Settings2,
  Sparkles,
  X,
} from "lucide-react";
import { useTranslation } from "react-i18next";

import type { SelectionTarget } from "../app/app-types";
import type { LearningPreferences } from "../learning/hooks/useLearningWorkspace";
import { SelectionPopoverSurface } from "./SelectionPopoverSurface";
import { selectionAiConfigured } from "./selection-ai";
import { useSelectionAiQuery } from "./useSelectionAiQuery";

const QUICK_QUESTION_KEYS = ["meaning", "grammar", "expression"] as const;

export function SelectionAiPopover({
  target,
  preferences,
  compact = false,
  onBack,
  onConfigure,
  onClose,
}: {
  target: SelectionTarget;
  preferences: LearningPreferences;
  compact?: boolean;
  onBack?: () => void;
  onConfigure: () => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const configured = selectionAiConfigured(preferences);
  const query = useSelectionAiQuery(preferences);
  const [question, setQuestion] = useState("");
  const [lastQuestion, setLastQuestion] = useState("");
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    setQuestion("");
    setLastQuestion("");
    setCopied(false);
    query.reset();
  }, [target, query.reset]);

  const ask = (nextQuestion = question) => {
    const normalized = nextQuestion.trim();
    if (!normalized || query.status === "loading") return;
    setQuestion("");
    setLastQuestion(normalized);
    setCopied(false);
    void query.ask(target, normalized);
  };

  const copyAnswer = async () => {
    if (!query.response) return;
    try {
      await navigator.clipboard.writeText(query.response.answer);
      setCopied(true);
    } catch {
      setCopied(false);
    }
  };

  return (
    <SelectionPopoverSurface
      target={target}
      compact={compact}
      className="selection-ai-popover"
      width={440}
      height={460}
      label={t("selection.ai.dialogLabel", { text: target.selectedText })}
      onClose={onClose}
    >
      <header className="selection-panel-header">
        {onBack && <button type="button" aria-label={t("selection.back")} onClick={onBack}><ArrowLeft size={18} /></button>}
        <span className="selection-panel-icon" aria-hidden="true"><Sparkles size={16} /></span>
        <div>
          <h2>{t("selection.ai.title")}</h2>
          <p title={target.selectedText}>{target.selectedText}</p>
        </div>
        <button type="button" aria-label={t("selection.close")} onClick={onClose}><X size={18} /></button>
      </header>

      {!configured ? (
        <div className="selection-ai-unconfigured">
          <Settings2 size={22} aria-hidden="true" />
          <strong>{t("selection.ai.configureTitle")}</strong>
          <p>{t("selection.ai.configureDescription")}</p>
          <button type="button" onClick={onConfigure}>{t("selection.ai.openSettings")}</button>
        </div>
      ) : (
        <>
          <div className="selection-ai-scroll">
            <div className="selection-ai-context">
              <span>{t("selection.ai.selectedText")}</span>
              <q>{target.selectedText}</q>
            </div>

            {query.response && (
              <section className="selection-ai-answer" aria-live="polite">
                <div className="selection-ai-answer-heading">
                  <strong>{t("selection.ai.answer")}</strong>
                  <span>{query.response.provider} · {query.response.model}</span>
                </div>
                <p lang={preferences.explanationLanguage}>{query.response.answer}</p>
                <div className="selection-ai-answer-actions">
                  <button type="button" onClick={() => void copyAnswer()}>
                    {copied ? <Check size={14} /> : <Copy size={14} />}
                    {t(copied ? "selection.ai.copied" : "selection.ai.copy")}
                  </button>
                  <button type="button" onClick={() => ask(lastQuestion)}><RotateCcw size={14} />{t("selection.ai.retry")}</button>
                </div>
              </section>
            )}

            {query.status === "loading" && (
              <div className="selection-ai-loading" role="status">
                <LoaderCircle className="spinning" size={18} />
                <span>{t("selection.ai.thinking")}</span>
              </div>
            )}
            {query.error && <p className="selection-ai-error" role="alert">{query.error}</p>}
          </div>

          <form className="selection-ai-form" onSubmit={(event) => { event.preventDefault(); ask(); }}>
            <div className="selection-ai-quick-actions" aria-label={t("selection.ai.quickQuestions")}>
              {QUICK_QUESTION_KEYS.map((key) => {
                const text = t(`selection.ai.quick.${key}`);
                return <button key={key} type="button" disabled={query.status === "loading"} onClick={() => ask(text)}>{text}</button>;
              })}
            </div>
            <div className="selection-ai-input-row">
              <textarea
                value={question}
                maxLength={1000}
                rows={2}
                placeholder={t("selection.ai.placeholder")}
                aria-label={t("selection.ai.questionLabel")}
                onChange={(event) => setQuestion(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
                    event.preventDefault();
                    ask();
                  }
                }}
              />
              <button type="submit" disabled={!question.trim() || query.status === "loading"} aria-label={t("selection.ai.send")}>
                {query.status === "loading" ? <LoaderCircle className="spinning" size={17} /> : <Send size={17} />}
              </button>
            </div>
          </form>
        </>
      )}
    </SelectionPopoverSurface>
  );
}
