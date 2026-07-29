import { useTranslation } from "react-i18next";
import { Minus, Square, X } from "lucide-react";

export function WindowChrome() {
  const { t } = useTranslation();
  const runWindowAction = async (action: "minimize" | "maximize" | "close") => {
    try {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      const appWindow = getCurrentWindow();
      if (action === "minimize") await appWindow.minimize();
      if (action === "maximize") await appWindow.toggleMaximize();
      if (action === "close") await appWindow.close();
    } catch {
      // Window controls are intentionally inactive in the browser preview.
    }
  };

  return (
    <header className="window-chrome" data-tauri-drag-region aria-label={t("window.controls")}>
      <div className="window-drag-region" data-tauri-drag-region />
      <div className="window-actions">
        <button type="button" aria-label={t("window.minimize")} title={t("window.minimizeShort")} onClick={() => void runWindowAction("minimize")}><Minus size={15} strokeWidth={1.8} /></button>
        <button type="button" aria-label={t("window.maximize")} title={t("window.maximizeShort")} onClick={() => void runWindowAction("maximize")}><Square size={12} strokeWidth={1.7} /></button>
        <button className="window-close" type="button" aria-label={t("window.close")} title={t("common.close")} onClick={() => void runWindowAction("close")}><X size={15} strokeWidth={1.8} /></button>
      </div>
    </header>
  );
}
