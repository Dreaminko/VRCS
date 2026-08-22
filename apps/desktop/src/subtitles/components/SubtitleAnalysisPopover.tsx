import { useEffect, useRef, useState } from "react";
import type { CSSProperties, RefObject } from "react";
import { createPortal } from "react-dom";
import { BookOpenText, Sparkles, X } from "lucide-react";
import { useTranslation } from "react-i18next";

import {
  interfaceLayoutPixels,
  readAppliedInterfaceScaleFactor,
} from "../../app/interface-scale";
import { isLookupAnchorVisible, placeLookupPopover } from "../../shared/lib/popover-placement";
import type { LearningAnalysis } from "../../learning/types";
import { LearningAnalysisView } from "../../learning/components/LearningAnalysisView";

const ANALYSIS_POPOVER_HEIGHT = 520;

export function SubtitleAnalysisPopover({
  anchorRef,
  analysis,
  onOpenLearning,
  onClose,
}: {
  anchorRef: RefObject<HTMLElement | null>;
  analysis: LearningAnalysis;
  onOpenLearning?: () => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const popoverRef = useRef<HTMLDivElement>(null);
  const [anchor, setAnchor] = useState(() => anchorForElement(anchorRef.current));
  const scale = readAppliedInterfaceScaleFactor();
  const viewportWidth = interfaceLayoutPixels(window.innerWidth, scale);
  const viewportHeight = interfaceLayoutPixels(window.innerHeight, scale);
  const layoutAnchor = {
    top: interfaceLayoutPixels(anchor.top, scale),
    bottom: interfaceLayoutPixels(anchor.bottom, scale),
    centerX: interfaceLayoutPixels(anchor.centerX, scale),
  };
  const width = Math.min(440, viewportWidth - 24);
  const placement = placeLookupPopover({
    anchor: layoutAnchor,
    popoverHeight: ANALYSIS_POPOVER_HEIGHT,
    viewportHeight,
    viewportTop: 40,
  });
  const left = Math.min(
    Math.max(12, layoutAnchor.centerX - width / 2),
    viewportWidth - width - 12,
  );
  const arrowLeft = Math.min(Math.max(22, layoutAnchor.centerX - left - 8), width - 38);

  useEffect(() => {
    const updateAnchor = () => {
      const rect = anchorRef.current?.getBoundingClientRect();
      if (!rect || !isLookupAnchorVisible(rect, window.innerWidth, window.innerHeight, 40)) {
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
  }, [anchorRef, onClose]);

  useEffect(() => {
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    const closeOnOutside = (event: PointerEvent) => {
      if (popoverRef.current && !popoverRef.current.contains(event.target as Node)) onClose();
    };
    document.addEventListener("keydown", closeOnEscape);
    document.addEventListener("pointerdown", closeOnOutside);
    return () => {
      document.removeEventListener("keydown", closeOnEscape);
      document.removeEventListener("pointerdown", closeOnOutside);
    };
  }, [onClose]);

  return createPortal(
    <div
      ref={popoverRef}
      className={`subtitle-analysis-popover popover-${placement.side}`}
      role="dialog"
      aria-label={t("live.contextMenu.quickAnalysisTitle")}
      style={{
        top: placement.top,
        left,
        width,
        height: placement.height,
        "--arrow-left": `${arrowLeft}px`,
      } as CSSProperties}
    >
      <header className="subtitle-analysis-popover-header">
        <div className="subtitle-analysis-popover-title">
          <span aria-hidden="true"><Sparkles size={16} /></span>
          <div>
            <h2>{t("live.contextMenu.quickAnalysisTitle")}</h2>
            <p>{analysis.provider} · {analysis.model} · {t("learning.analysis.confidence", { value: String(analysis.confidence) })}</p>
          </div>
        </div>
        <button type="button" aria-label={t("live.contextMenu.closeAnalysis")} onClick={onClose}><X size={18} /></button>
      </header>
      <div className="subtitle-analysis-popover-scroll">
        <LearningAnalysisView analysis={analysis} showHeading={false} />
      </div>
      {onOpenLearning && (
        <footer className="subtitle-analysis-popover-actions">
          <button type="button" onClick={() => {
            onClose();
            onOpenLearning();
          }}><BookOpenText size={15} />{t("live.contextMenu.openFullAnalysis")}</button>
        </footer>
      )}
      <i className="popover-arrow" aria-hidden="true" />
    </div>,
    document.body,
  );
}

function anchorForElement(element: HTMLElement | null) {
  const rect = element?.getBoundingClientRect();
  if (!rect) return { top: 80, bottom: 80, centerX: window.innerWidth / 2 };
  return { top: rect.top, bottom: rect.bottom, centerX: rect.left + rect.width / 2 };
}
