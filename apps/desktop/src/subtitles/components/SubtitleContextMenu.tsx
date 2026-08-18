import { useEffect, useRef } from "react";
import type { CSSProperties, KeyboardEvent } from "react";
import { createPortal } from "react-dom";
import { BookOpenText, Check, Copy, Languages, LoaderCircle, Sparkles, X } from "lucide-react";
import { useTranslation } from "react-i18next";

import { useDismissibleLayer } from "../../shared/hooks/use-dismissible-layer";

type ContextMenuPosition = {
  top: number;
  left: number;
  side: "above" | "below";
};

export function SubtitleContextMenu({
  position,
  selectedText,
  translation,
  translating,
  learningBusy,
  learningCaptured,
  analyzing,
  analysisComplete,
  selectionCount,
  onCopySelection,
  onCopyOriginal,
  onCopyTranslation,
  onCopyBilingual,
  onTranslate,
  onAnalyze,
  onLearning,
  onClearSelection,
  onClose,
}: {
  position: ContextMenuPosition;
  selectedText: string;
  translation: string | null;
  translating: boolean;
  learningBusy: boolean;
  learningCaptured: boolean;
  analyzing: boolean;
  analysisComplete: boolean;
  selectionCount?: number;
  onCopySelection: () => void;
  onCopyOriginal: () => void;
  onCopyTranslation: () => void;
  onCopyBilingual: () => void;
  onTranslate?: () => void;
  onAnalyze?: () => void;
  onLearning?: () => void;
  onClearSelection?: () => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const menuRef = useRef<HTMLDivElement>(null);
  const selectionMode = selectionCount !== undefined;

  useDismissibleLayer(true, menuRef, onClose);

  useEffect(() => {
    menuRef.current?.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
    const closeForViewportChange = () => onClose();
    window.addEventListener("resize", closeForViewportChange);
    window.addEventListener("scroll", closeForViewportChange, true);
    return () => {
      window.removeEventListener("resize", closeForViewportChange);
      window.removeEventListener("scroll", closeForViewportChange, true);
    };
  }, [onClose]);

  const choose = (action: () => void) => {
    onClose();
    action();
  };

  return createPortal(
    <div
      ref={menuRef}
      className={`subtitle-context-menu ${position.side === "above" ? "above" : ""}`}
      role="menu"
      aria-label={t("live.contextMenu.label")}
      style={{ top: position.top, left: position.left } as CSSProperties}
      onKeyDown={moveMenuFocus}
    >
      {selectionMode && (
        <div className="subtitle-context-menu-summary">
          {t("live.contextMenu.selectedSubtitles", { count: selectionCount })}
        </div>
      )}
      {selectedText && (
        <button type="button" role="menuitem" onClick={() => choose(onCopySelection)}>
          <Copy size={15} />{t("live.contextMenu.copySelection")}
        </button>
      )}
      <button type="button" role="menuitem" onClick={() => choose(onCopyOriginal)}>
        <Copy size={15} />{t(selectionMode ? "live.contextMenu.copySelectedOriginal" : "live.contextMenu.copyOriginal")}
      </button>
      {translation && (
        <>
          <button type="button" role="menuitem" onClick={() => choose(onCopyTranslation)}>
            <Copy size={15} />{t(selectionMode ? "live.contextMenu.copySelectedTranslation" : "live.contextMenu.copyTranslation")}
          </button>
          <button type="button" role="menuitem" onClick={() => choose(onCopyBilingual)}>
            <Languages size={15} />{t(selectionMode ? "live.contextMenu.copySelectedBilingual" : "live.contextMenu.copyBilingual")}
          </button>
        </>
      )}
      {(onTranslate || onAnalyze || onLearning) && <div className="subtitle-context-menu-separator" role="separator" />}
      {onTranslate && (
        <button type="button" role="menuitem" disabled={translating} onClick={() => choose(onTranslate)}>
          {translating ? <LoaderCircle className="spinning" size={15} /> : <Languages size={15} />}
          {t(translating ? "translation.translating" : translation ? "translation.retry" : "translation.action")}
        </button>
      )}
      {onAnalyze && (
        <button type="button" role="menuitem" disabled={analyzing} onClick={() => choose(onAnalyze)}>
          {analyzing ? <LoaderCircle className="spinning" size={15} /> : analysisComplete ? <Check size={15} /> : <Sparkles size={15} />}
          {t(analyzing
            ? "live.contextMenu.analyzing"
            : analysisComplete
              ? selectionMode ? "live.contextMenu.reanalyzeSelected" : "live.contextMenu.reanalyze"
              : selectionMode ? "live.contextMenu.analyzeSelected" : "live.contextMenu.analyze")}
        </button>
      )}
      {onLearning && (
        <button type="button" role="menuitem" disabled={learningBusy} onClick={() => choose(onLearning)}>
          {learningBusy ? <LoaderCircle className="spinning" size={15} /> : learningCaptured ? <Check size={15} /> : <BookOpenText size={15} />}
          {t(learningBusy
            ? "learning.actions.adding"
            : learningCaptured
              ? "live.contextMenu.openLearning"
              : selectionMode ? "live.contextMenu.addSelectedLearning" : "learning.actions.add")}
        </button>
      )}
      {onClearSelection && (
        <>
          <div className="subtitle-context-menu-separator" role="separator" />
          <button type="button" role="menuitem" onClick={() => choose(onClearSelection)}>
            <X size={15} />{t("live.contextMenu.clearSelection")}
          </button>
        </>
      )}
    </div>,
    document.body,
  );
}

function moveMenuFocus(event: KeyboardEvent<HTMLDivElement>) {
  const buttons = [...event.currentTarget.querySelectorAll<HTMLButtonElement>("button:not(:disabled)")];
  if (!buttons.length) return;
  const currentIndex = buttons.indexOf(document.activeElement as HTMLButtonElement);
  let nextIndex = currentIndex;
  if (event.key === "ArrowDown") nextIndex = (currentIndex + 1) % buttons.length;
  else if (event.key === "ArrowUp") nextIndex = (currentIndex - 1 + buttons.length) % buttons.length;
  else if (event.key === "Home") nextIndex = 0;
  else if (event.key === "End") nextIndex = buttons.length - 1;
  else return;
  event.preventDefault();
  buttons[nextIndex]?.focus();
}
