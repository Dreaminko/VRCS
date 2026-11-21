import { useTranslation } from "react-i18next";

import type {
  RecognitionActions,
  RecognitionModels,
  SaveState,
} from "../settings-types";
import type { Settings } from "../types";
import { OcrModelManagerPanel } from "../ocr/OcrModelManagerPanel";
import { ModelDirectorySetting } from "../recognition/ModelDirectorySetting";
import { ModelManagerPanel } from "../recognition/ModelManagerPanel";
import { LocalTranslationPanel } from "../translation/LocalTranslationPanel";
import { CapabilitySection, usePlatformCapabilities } from "../../core-client/capabilities";

export function LocalModelsSection({
  settings,
  locale,
  disabled,
  saveState,
  models,
  actions,
}: {
  settings: Settings;
  locale: string;
  disabled: boolean;
  saveState: SaveState;
  models: RecognitionModels;
  actions: RecognitionActions;
}) {
  const { t } = useTranslation();
  const { core } = usePlatformCapabilities();
  const qwenDownloading = models.qwen.some((model) =>
    model.status === "downloading" || model.status === "verifying");
  const runtimeInstalling = models.qwenRuntime?.installation
    && ["downloading", "verifying"].includes(models.qwenRuntime.installation.status);
  return <div className="model-assignments-section local-models-section settings-section-active">
    <div className="local-model-category global-model-directory">
      <ModelDirectorySetting
        directoryText={models.directoryText}
        saveState={saveState}
        disabled={disabled}
        busy={Boolean(qwenDownloading || runtimeInstalling)}
        title={t("settings.modelAssignments.globalModelDirectory")}
        description={t("settings.modelAssignments.globalModelDirectoryDescription")}
        onSetDirectoryText={actions.setModelDirectoryText}
        onUpdateDirectory={actions.updateModelDirectory}
        onChooseDirectory={actions.chooseModelDirectory}
      />
    </div>
    <div className="local-model-category">
      <CapabilitySection capability={core?.inference.qwen_cpu}>
      <ModelManagerPanel
        qwen={{
          models: models.qwen,
          ready: models.qwenReady,
          message: models.qwenMessage,
          selectedId: settings.asr.backend === "qwen_local_managed"
            ? settings.asr.managed_qwen.package_id
            : null,
          onDownload: actions.downloadQwenModel,
          onCancel: actions.cancelQwenDownload,
          onRemove: actions.removeQwenModel,
        }}
        runtime={models.qwenRuntime}
        onDownloadRuntime={actions.downloadQwenRuntime}
        onCancelRuntimeDownload={actions.cancelQwenRuntimeDownload}
        title={t("settings.recognition.qwenModels")}
        locale={locale}
        disabled={disabled}
        directoryText={models.directoryText}
        saveState={saveState}
        onSetDirectoryText={actions.setModelDirectoryText}
        onUpdateDirectory={actions.updateModelDirectory}
        onChooseDirectory={actions.chooseModelDirectory}
        showDirectory={false}
      />
      </CapabilitySection>
    </div>
    <div className="local-model-category">
      <CapabilitySection capability={core?.inference.local_translation}>
      <LocalTranslationPanel locale={locale} disabled={disabled} />
      </CapabilitySection>
    </div>
    <div className="local-model-category">
      <OcrModelManagerPanel locale={locale} disabled={disabled} />
    </div>
  </div>;
}
