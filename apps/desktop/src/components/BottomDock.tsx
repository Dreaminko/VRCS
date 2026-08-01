import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { History, MessageSquare, Mic, Shrink, SlidersHorizontal, Square } from "lucide-react";

import type { Page } from "../app-types";

export function BottomDock({ page, running, captureDisabled, onPageChange, onCompact, onCapture }: {
  page: Page;
  running: boolean;
  captureDisabled: boolean;
  onPageChange: (page: Page) => void;
  onCompact: () => void;
  onCapture: () => void;
}) {
  const { t } = useTranslation();
  return (
    <nav className="bottom-dock" aria-label={t("navigation.main")}>
      <DockButton label={t("navigation.live")} active={page === "live"} onClick={() => onPageChange("live")}><MessageSquare /></DockButton>
      <DockButton label={t("navigation.history")} active={page === "history"} onClick={() => onPageChange("history")}><History /></DockButton>
      <DockButton label={t("navigation.settings")} active={page === "settings"} onClick={() => onPageChange("settings")}><SlidersHorizontal /></DockButton>
      <i className="dock-divider" aria-hidden="true" />
      <DockButton label={t("navigation.compact")} tonal onClick={onCompact}><Shrink /></DockButton>
      <DockButton label={t(running ? "capture.stop" : "capture.start")} primary disabled={captureDisabled} onClick={onCapture}>{running ? <Square /> : <Mic />}</DockButton>
    </nav>
  );
}

function DockButton({ label, active = false, tonal = false, primary = false, disabled = false, onClick, children }: {
  label: string;
  active?: boolean;
  tonal?: boolean;
  primary?: boolean;
  disabled?: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      className={`dock-button ${active ? "active" : ""} ${tonal ? "tonal" : ""} ${primary ? "primary" : ""}`}
      aria-label={label}
      aria-current={active ? "page" : undefined}
      data-tooltip={label}
      title={label}
      disabled={disabled}
      onClick={onClick}
    >{children}</button>
  );
}
