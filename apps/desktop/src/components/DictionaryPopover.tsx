import { useEffect, useRef, useState } from "react";
import type { CSSProperties } from "react";
import { useTranslation } from "react-i18next";
import { Check, PlusCircle, TriangleAlert, X } from "lucide-react";

import { coreApi } from "../api";
import { ankiButtonLabel } from "../anki";
import type { AnkiAddState } from "../anki";
import { contextExcerpt, localizedError } from "../app-utils";
import type { Lookup } from "../app-types";
import { ankiDictionaryContent, definitionGlosses, groupDictionaryEntries } from "../dictionary";
import {
  interfaceLayoutPixels,
  readAppliedInterfaceScaleFactor,
} from "../interface-scale";
import {
  isLookupAnchorVisible,
  LOOKUP_POPOVER_HEIGHT,
  placeLookupPopover,
} from "../popover-placement";

export function DictionaryPopover({ lookup, ankiEnabled, compact = false, onClose }: { lookup: Lookup; ankiEnabled: boolean; compact?: boolean; onClose: () => void }) {
  const { t } = useTranslation();
  const ref = useRef<HTMLDivElement>(null);
  const [ankiState, setAnkiState] = useState<AnkiAddState>("idle");
  const [ankiFeedback, setAnkiFeedback] = useState("");
  const [anchor, setAnchor] = useState(lookup.anchor);
  const groupedEntries = groupDictionaryEntries(lookup.entries);
  const entry = groupedEntries[0];
  const visibleEntries = groupedEntries.slice(0, 6);
  const scale = compact ? 1 : readAppliedInterfaceScaleFactor();
  const viewportWidth = interfaceLayoutPixels(window.innerWidth, scale);
  const viewportHeight = interfaceLayoutPixels(window.innerHeight, scale);
  const layoutAnchor = compact ? anchor : {
    top: interfaceLayoutPixels(anchor.top, scale),
    bottom: interfaceLayoutPixels(anchor.bottom, scale),
    centerX: interfaceLayoutPixels(anchor.centerX, scale),
  };
  const width = Math.min(340, viewportWidth - 24);
  const placement = placeLookupPopover({
    anchor: layoutAnchor,
    popoverHeight: LOOKUP_POPOVER_HEIGHT,
    viewportHeight,
    viewportTop: 40,
  });
  const left = Math.min(
    Math.max(12, layoutAnchor.centerX - 34),
    viewportWidth - width - 12,
  );
  const arrowLeft = Math.min(Math.max(22, layoutAnchor.centerX - left - 8), width - 38);
  const style = compact
    ? undefined
    : { left, top: placement.top, width, height: placement.height, "--arrow-left": `${arrowLeft}px` };

  useEffect(() => {
    setAnkiState("idle");
    setAnkiFeedback("");
  }, [lookup.term, lookup.context]);

  useEffect(() => {
    if (compact || !lookup.range) return;

    const updateAnchor = () => {
      const rect = lookup.range?.getBoundingClientRect();
      if (!rect || !isLookupAnchorVisible(
        rect,
        window.innerWidth,
        window.innerHeight,
        40,
      )) {
        onClose();
        return;
      }
      setAnchor({ top: rect.top, bottom: rect.bottom, centerX: rect.left + rect.width / 2 });
    };

    updateAnchor();
    window.addEventListener("scroll", updateAnchor, true);
    window.addEventListener("resize", updateAnchor);
    return () => {
      window.removeEventListener("scroll", updateAnchor, true);
      window.removeEventListener("resize", updateAnchor);
    };
  }, [compact, lookup.range, onClose]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => { if (event.key === "Escape") onClose(); };
    const onPointerDown = (event: PointerEvent) => {
      if (!compact && ref.current && !ref.current.contains(event.target as Node)) onClose();
    };
    document.addEventListener("keydown", onKeyDown);
    document.addEventListener("pointerdown", onPointerDown);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
      document.removeEventListener("pointerdown", onPointerDown);
    };
  }, [compact, onClose]);

  const add = async () => {
    if (!entry || ankiState === "adding") return;
    const cardContent = ankiDictionaryContent(visibleEntries, t("dictionary.builtIn"));
    setAnkiState("adding");
    setAnkiFeedback("");
    try {
      const result = await coreApi.createCard({
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

  return (
    <div ref={ref} className={`dictionary-popover ${compact ? "compact-inline-dictionary" : `popover-${placement.side}`}`} style={style as CSSProperties} role="dialog" aria-label={t("dictionary.dialogLabel", { term: lookup.term })}>
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
        {visibleEntries.length ? (
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
      {ankiEnabled && (
        <>
          <button className={`anki-button anki-state-${ankiState}`} type="button" disabled={!entry || ankiState === "adding" || ankiState === "success"} onClick={() => void add()}>
            {ankiState === "success"
              ? <Check size={16} />
              : ankiState === "error"
                ? <TriangleAlert size={16} />
                : <PlusCircle size={16} />}
            {ankiButtonLabel(ankiState, (key) => t(key))}
          </button>
          {ankiFeedback && (
            <p className={`dictionary-anki-feedback ${ankiState}`} role={ankiState === "error" ? "alert" : "status"}>
              {ankiFeedback}
            </p>
          )}
        </>
      )}
      {!compact && <i className="popover-arrow" aria-hidden="true" />}
    </div>
  );
}
