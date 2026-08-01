import { Clock3 } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { Settings } from "../../types";
import { RangeField } from "../SettingsControls";

export function VadSettings({
  vad,
  disabled,
  onUpdate,
}: {
  vad: Settings["vad"];
  disabled: boolean;
  onUpdate: <K extends keyof Settings["vad"]>(key: K, value: Settings["vad"][K]) => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="recognition-config-row">
      <div className="recognition-config-title">
        <Clock3 size={17} />
        <span><strong>{t("settings.recognition.segmentation")}</strong><small>{t("settings.recognition.segmentationDescription")}</small></span>
      </div>
      <div className="recognition-config-fields">
        <RangeField
          label={t("settings.recognition.silence")}
          helper={t("settings.recognition.silenceDescription")}
          value={vad.silence_seconds}
          min={0.1}
          max={2}
          step={0.1}
          disabled={disabled}
          formatValue={(value) => t("units.seconds", { value: value.toFixed(1) })}
          onCommit={(value) => onUpdate("silence_seconds", value)}
        />
        <RangeField
          label={t("settings.recognition.maxSegment")}
          helper={t("settings.recognition.maxSegmentDescription")}
          value={vad.max_speech_seconds}
          min={1}
          max={30}
          step={1}
          disabled={disabled}
          formatValue={(value) => t("units.seconds", { value })}
          onCommit={(value) => onUpdate("max_speech_seconds", value)}
        />
      </div>
    </div>
  );
}
