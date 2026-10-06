import { useEffect, useState } from "react";
import { ScanText } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { VrOcrStatus } from "../../integrations/types";
import type { ApiProfileView } from "../../providers/types";
import type { Settings } from "../types";
import type { ApplySettings, SaveState } from "../settings-types";
import { getVrOverlayStatus, listenVrOverlayStatus } from "../../vr-overlay-native";
import { getDesktopOcrStatus, listenDesktopOcrStatus } from "../../ocr/api";
import { applyDesktopOcrStatus, type DesktopOcrStatus } from "../../ocr/status";
import { patchOcr } from "../vr-overlay-settings";
import { OcrSettingsCard } from "./OcrSettingsCard";

export function OcrSettingsSection({ draft, profiles, saveState, applySettings }: {
  draft: Settings; profiles: ApiProfileView[]; saveState: SaveState; applySettings: ApplySettings;
}) {
  const { t } = useTranslation();
  const [runtime, setRuntime] = useState<VrOcrStatus>();
  const [desktopStatus, setDesktopStatus] = useState<DesktopOcrStatus | null>(null);
  useEffect(() => {
    let disposed = false;
    const stops: Array<() => void> = [];
    const subscribe = async () => {
      const vrStop = await listenVrOverlayStatus((next) => { if (!disposed) setRuntime(next.ocr); });
      if (disposed) { vrStop(); return; }
      stops.push(vrStop);
      const desktopStop = await listenDesktopOcrStatus((next) => {
        if (!disposed) setDesktopStatus((current) => applyDesktopOcrStatus(current, next));
      });
      if (disposed) { desktopStop(); return; }
      stops.push(desktopStop);
      const [vr, desktop] = await Promise.all([getVrOverlayStatus(), getDesktopOcrStatus()]);
      if (!disposed) {
        setRuntime(vr.ocr);
        setDesktopStatus((current) => applyDesktopOcrStatus(current, desktop));
      }
    };
    void subscribe().catch(() => undefined);
    return () => { disposed = true; stops.forEach((stop) => stop()); };
  }, []);
  return <div className="settings-section settings-section-active vr-overlay-section ocr-settings-section"
    id="settings-panel-ocr" role="tabpanel" aria-labelledby="settings-tab-ocr">
    <div className="section-heading"><div><ScanText size={18} /><h2>{t("settings.categories.ocr")}</h2></div></div>
    <OcrSettingsCard config={draft.ocr} profiles={profiles} disabled={saveState === "saving"}
      runtime={runtime} desktopStatus={desktopStatus}
      onChange={(patch) => applySettings((current) => patchOcr(current, patch))} />
  </div>;
}
