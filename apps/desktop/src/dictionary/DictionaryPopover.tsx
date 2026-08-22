import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { BookOpenText, Check, LoaderCircle, PlusCircle, Sparkles, TriangleAlert, X } from "lucide-react";

import { ankiApi } from "../anki/api";
import { ankiButtonLabel } from "../anki/anki";
import type { AnkiAddState } from "../anki/anki";
import { contextExcerpt, localizedError } from "../app/app-utils";
import type { Lookup } from "../app/app-types";
import { ankiDictionaryContent, definitionGlosses, groupDictionaryEntries } from "./dictionary";
import { LOOKUP_POPOVER_HEIGHT } from "../shared/lib/popover-placement";
import { SelectionPopoverSurface } from "../selection/SelectionPopoverSurface";

export function DictionaryPopover({ lookup, loading = false, ankiEnabled, compact = false, onAskAi, onAddLearning, onClose }: { lookup: Lookup; loading?: boolean; ankiEnabled: boolean; compact?: boolean; onAskAi: () => void; onAddLearning?: (lookup: Lookup) => Promise<unknown>; onClose: () => void }) {
  const { t } = useTranslation();
  const [ankiState, setAnkiState] = useState<AnkiAddState>("idle");
  const [ankiFeedback, setAnkiFeedback] = useState("");
  const [learningState, setLearningState] = useState<"idle" | "adding" | "success" | "error">("idle");
  const [learningFeedback, setLearningFeedback] = useState("");
  const groupedEntries = groupDictionaryEntries(lookup.entries);
  const entry = groupedEntries[0];
  const visibleEntries = groupedEntries.slice(0, 6);

  useEffect(() => {
    setAnkiState("idle");
    setAnkiFeedback("");
    setLearningState("idle");
    setLearningFeedback("");
  }, [lookup.term, lookup.context]);

  const add = async () => {
    if (!entry || ankiState === "adding") return;
    const cardContent = ankiDictionaryContent(visibleEntries, t("dictionary.builtIn"));
    setAnkiState("adding");
    setAnkiFeedback("");
    try {
      const result = await ankiApi.createCard({
        term: lookup.term,
        reading: entry.reading,
        definition: cardContent.definition,
        context: lookup.context,
        dictionary: cardContent.dictionary,
        language: entry.language,
        labels: {
          definition: t("anki.card.definition"),
          context: t("anki.card.context"),
        },
      });
      setAnkiState("success");
      setAnkiFeedback(t("dictionary.anki.created", {
        noteId: result.note_id,
        count: visibleEntries.length,
      }));
    } catch (reason) {
      setAnkiState("error");
      setAnkiFeedback(localizedError(reason, t, "errors.anki.createCard"));
    }
  };

  const addLearning = async () => {
    if (loading || !onAddLearning || learningState === "adding" || learningState === "success") return;
    setLearningState("adding");
    setLearningFeedback("");
    try {
      const result = await onAddLearning(lookup);
      if (!result) {
        setLearningState("error");
        setLearningFeedback(t("dictionary.learning.failed"));
        return;
      }
      setLearningState("success");
      setLearningFeedback(t("dictionary.learning.saved"));
    } catch {
      setLearningState("error");
      setLearningFeedback(t("dictionary.learning.failed"));
    }
  };

  return (
    <SelectionPopoverSurface
      target={lookup}
      compact={compact}
      className="dictionary-popover"
      width={340}
      height={LOOKUP_POPOVER_HEIGHT}
      label={t("dictionary.dialogLabel", { term: lookup.term })}
      onClose={onClose}
    >
      <div className="dictionary-header">
        <div className="dictionary-title-block">
          <div className="dictionary-title-copy">
            <div className="dictionary-term-row"><h2>{lookup.term}</h2>{entry && <span className="language-chip">{entry.language.toUpperCase()}</span>}</div>
            {entry?.reading && <span className="reading">{entry.reading}</span>}
          </div>
        </div>
        <button type="button" aria-label={t("dictionary.close")} onClick={onClose}><X size={19} /></button>
      </div>
      <div className="dictionary-scroll">
        {loading ? (
          <div className="dictionary-loading" role="status">
            <LoaderCircle className="spinning" size={18} />
            <span>{t("selection.dictionaryLoading")}</span>
          </div>
        ) : visibleEntries.length ? (
          <div className="dictionary-definitions">
            {visibleEntries.map((item, index) => (
              <article className="dictionary-definition-item" key={`${item.dictionary ?? "local"}-${item.term}-${item.reading ?? ""}-${index}`}>
                <div className="dictionary-entry-meta">
                  <span className="dictionary-source-name">{item.dictionary || t("dictionary.builtIn")}</span>
                  {visibleEntries.length > 1 && <span className="dictionary-entry-index">{String(index + 1).padStart(2, "0")}</span>}
                </div>
                <ol className="definition-glosses">
                  {definitionGlosses(item.definition).map((gloss, glossIndex) => <li key={`${gloss}-${glossIndex}`}>{gloss}</li>)}
                </ol>
              </article>
            ))}
          </div>
        ) : <p className="definition muted">{t("dictionary.noDefinitions")}</p>}
        <div className="lookup-context"><span>{t("dictionary.context")}</span><q>{contextExcerpt(lookup.context, lookup.term)}</q></div>
      </div>
      <div className="dictionary-action-area">
        <div className="dictionary-action-row">
          <button className="dictionary-ai-button" type="button" onClick={onAskAi}>
            <Sparkles size={15} />
            {t("selection.askAi")}
          </button>
          {onAddLearning && (
              <button className={`dictionary-learning-button learning-state-${learningState}`} type="button" disabled={loading || learningState === "adding" || learningState === "success"} onClick={() => void addLearning()}>
                {learningState === "success" ? <Check size={15} /> : learningState === "error" ? <TriangleAlert size={15} /> : <BookOpenText size={15} />}
                {t(learningState === "success" ? "dictionary.learning.savedAction" : learningState === "adding" ? "dictionary.learning.saving" : learningState === "error" ? "dictionary.learning.retry" : "dictionary.learning.add")}
              </button>
          )}
          {ankiEnabled && (
            <button className={`anki-button anki-state-${ankiState}`} type="button" disabled={!entry || ankiState === "adding" || ankiState === "success"} onClick={() => void add()}>
              {ankiState === "success"
                ? <Check size={16} />
                : ankiState === "error"
                  ? <TriangleAlert size={16} />
                  : <PlusCircle size={16} />}
              {ankiButtonLabel(ankiState, (key) => t(key))}
            </button>
          )}
        </div>
        {learningFeedback && <p className={`dictionary-anki-feedback ${learningState}`} role={learningState === "error" ? "alert" : "status"}>{learningFeedback}</p>}
        {ankiFeedback && (
          <p className={`dictionary-anki-feedback ${ankiState}`} role={ankiState === "error" ? "alert" : "status"}>
            {ankiFeedback}
          </p>
        )}
      </div>
    </SelectionPopoverSurface>
  );
}
