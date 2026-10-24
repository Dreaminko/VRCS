import { useEffect, useState } from "react";
import { Gamepad2, ScanEye } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { VrOcrStatus } from "../../integrations/types";
import type { ApiProfileView, ProviderDefinition } from "../../providers/types";
import type { Settings } from "../types";
import type { ApplySettings } from "../settings-types";
import { getVrOverlayStatus, listenVrOverlayStatus, openVrOcrBindings } from "../../integrations/vr-overlay-native";
import { getDesktopOcrStatus, listenDesktopOcrStatus } from "../../ocr/api";
import { applyDesktopOcrStatus, type DesktopOcrStatus } from "../../ocr/status";
import { patchOcr } from "../vr-overlay-settings";
import { OcrSettingsCard } from "./OcrSettingsCard";
import { VisionSettingsCard } from "./VisionSettingsCard";
import { patchVision, selectVisionProfile } from "../vision-settings";
import { PreferenceToggle } from "../SettingsControls";

export function OcrSettingsSection({ vrAvailable, draft, profiles, providerDefinitions, applySettings }: {
  vrAvailable: boolean; draft: Settings; profiles: ApiProfileView[]; providerDefinitions: ProviderDefinition[]; applySettings: ApplySettings;
}) {
  const { t } = useTranslation();
  const [runtime, setRuntime] = useState<VrOcrStatus>();
  const [desktopStatus, setDesktopStatus] = useState<DesktopOcrStatus | null>(null);
  const [bindingError, setBindingError] = useState("");
  const vrInputEnabled = draft.features.vr_overlay && (
    (draft.features.ocr && draft.ocr.master_enabled && draft.ocr.enabled)
    || (draft.features.visual_question_answering && draft.vision.enabled && draft.vision.vr_enabled)
  );
  useEffect(() => {
    if (!draft.features.ocr) return;
    let disposed = false;
    const stops: Array<() => void> = [];
    const subscribe = async () => {
      if (vrAvailable) {
        const vrStop = await listenVrOverlayStatus((next) => { if (!disposed) setRuntime(next.ocr); });
        if (disposed) { vrStop(); return; }
        stops.push(vrStop);
      }
      const desktopStop = await listenDesktopOcrStatus((next) => {
        if (!disposed) setDesktopStatus((current) => applyDesktopOcrStatus(current, next));
      });
      if (disposed) { desktopStop(); return; }
      stops.push(desktopStop);
      const [vr, desktop] = await Promise.all([vrAvailable ? getVrOverlayStatus() : undefined, getDesktopOcrStatus()]);
      if (!disposed) {
        setRuntime(vr?.ocr);
        setDesktopStatus((current) => applyDesktopOcrStatus(current, desktop));
      }
    };
    void subscribe().catch(() => undefined);
    return () => { disposed = true; stops.forEach((stop) => stop()); };
  }, [vrAvailable, draft.features.ocr]);
  return <div className="settings-section settings-section-active vr-overlay-section ocr-settings-section"
    id="settings-panel-vision" role="tabpanel" aria-labelledby="settings-tab-vision">
    <div className="section-heading"><div><ScanEye size={18} /><h2>{t("settings.categories.vision")}</h2></div></div>
    {vrAvailable && <section className="vr-overlay-card" aria-label={t("settings.vrOcr.gesture")}>
      <PreferenceToggle title={t("settings.vrOcr.gesture")} checked={draft.ocr.hand_gesture_enabled}
        description={t("settings.vrOcr.gestureDescription")}
        disabled={false} onChange={(hand_gesture_enabled) => applySettings((current) => patchOcr(current, { hand_gesture_enabled }))} />
      <div className="vr-overlay-card-actions">
        <button className="secondary-button" type="button" disabled={!vrInputEnabled}
          onClick={() => { setBindingError(""); void openVrOcrBindings().catch(() => setBindingError(t("settings.vrOcr.bindingFailed"))); }}>
          <Gamepad2 size={15} />{t("settings.vrOcr.bindings")}
        </button>
      </div>
      {bindingError && <p className="vr-overlay-native-error" role="alert">{bindingError}</p>}
    </section>}
    {draft.features.ocr && <div id="vision-ocr"><OcrSettingsCard vrAvailable={vrAvailable} config={draft.ocr} profiles={profiles} disabled={false}
      runtime={runtime} desktopStatus={desktopStatus}
      onChange={(patch) => applySettings((current) => patchOcr(current, patch))} /></div>}
    {draft.features.visual_question_answering && <VisionSettingsCard config={draft.vision} profiles={profiles}
      recognition={draft.asr} providerDefinitions={providerDefinitions}
      vrAvailable={vrAvailable} disabled={false}
      onChange={(patch) => applySettings((current) => patchVision(current, patch))}
      onSelectProfile={(profileId) => applySettings((current) => selectVisionProfile(current, profileId))} />}
  </div>;
}
