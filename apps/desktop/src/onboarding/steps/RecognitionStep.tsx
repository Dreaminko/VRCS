import { Check, ChevronRight, Cloud, HardDrive, KeyRound, RefreshCw, ShieldCheck } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { ApiProfileEditorDraft } from "../../settings/api/ApiProfileEditor";
import { ApiProfileEditor } from "../../settings/api/ApiProfileEditor";
import type { useAsrModels } from "../../settings/hooks/useAsrModels";
import type { SettingsDraftController } from "../../settings/hooks/useSettingsDraft";
import { LocalRecognitionSettings, LocalRuntimeStatus } from "../../settings/recognition/LocalRecognitionSettings";
import { ModelManagerPanel } from "../../settings/recognition/ModelManagerPanel";
import { Select } from "../../settings/SettingsControls";
import { validComputeTypes } from "../../settings/settings-validation";
import type { useApiProfiles } from "../../settings/useApiProfiles";
import type { ApiProfileView, ApiProvider, AsrCapabilities } from "../../types";
import type { CloudBackend, RecognitionMode } from "../onboarding-types";

type AsrModelsController = ReturnType<typeof useAsrModels>;
type ApiProfilesController = ReturnType<typeof useApiProfiles>;

export function RecognitionStep({
  recognitionMode,
  cloudBackend,
  operationBusy,
  provider,
  recognitionProfiles,
  selectedProfileId,
  testedProfileId,
  selectedProfile,
  apiEditor,
  apiProfiles,
  draftController,
  asr,
  asrCapabilities,
  localSettingsError,
  localReady,
  locale,
  busy,
  onSetRecognitionMode,
  onSetCloudBackend,
  onSelectProfile,
  onAddApiProfile,
  onChangeApiEditor,
  onSaveApiEditor,
  onCancelApiEditor,
  onTestAndApplyCloud,
}: {
  recognitionMode: RecognitionMode;
  cloudBackend: CloudBackend;
  operationBusy: boolean;
  provider: ApiProvider;
  recognitionProfiles: ApiProfileView[];
  selectedProfileId: string;
  testedProfileId: string;
  selectedProfile: ApiProfileView | undefined;
  apiEditor: ApiProfileEditorDraft | null;
  apiProfiles: ApiProfilesController;
  draftController: SettingsDraftController;
  asr: AsrModelsController;
  asrCapabilities: AsrCapabilities | null;
  localSettingsError: string | null;
  localReady: boolean;
  locale: string;
  busy: boolean;
  onSetRecognitionMode: (mode: RecognitionMode) => void;
  onSetCloudBackend: (backend: CloudBackend) => void;
  onSelectProfile: (profileId: string) => void;
  onAddApiProfile: () => void;
  onChangeApiEditor: (draft: ApiProfileEditorDraft) => void;
  onSaveApiEditor: () => void;
  onCancelApiEditor: () => void;
  onTestAndApplyCloud: () => void;
}) {
  const { t } = useTranslation();

  return (
    <div className="onboarding-step-content">
      <div className="onboarding-intro"><p>{t("onboarding.recognition.description")}</p></div>
      <div className="onboarding-choice-grid">
        <button className={`onboarding-choice ${recognitionMode === "cloud" ? "selected" : ""}`} type="button" aria-pressed={recognitionMode === "cloud"} disabled={operationBusy} onClick={() => onSetRecognitionMode("cloud")}>
          <Cloud size={23} /><span><strong>{t("onboarding.recognition.cloud")}</strong><small>{t("onboarding.recognition.cloudDescription")}</small></span><i>{recognitionMode === "cloud" && <Check size={14} />}</i>
        </button>
        <button className={`onboarding-choice ${recognitionMode === "local" ? "selected" : ""}`} type="button" aria-pressed={recognitionMode === "local"} disabled={operationBusy} onClick={() => onSetRecognitionMode("local")}>
          <HardDrive size={23} /><span><strong>{t("onboarding.recognition.local")}</strong><small>{t("onboarding.recognition.localDescription")}</small></span><i>{recognitionMode === "local" && <Check size={14} />}</i>
        </button>
      </div>

      {recognitionMode === "cloud" ? (
        <div className="onboarding-config-panel">
          <div className="onboarding-panel-heading"><KeyRound size={18} /><div><strong>{t("onboarding.recognition.cloudSetup")}</strong><small>{t("settings.apiManagement.securityNotice")}</small></div></div>
          <Select
            label={t("settings.recognition.cloudService")}
            value={cloudBackend}
            options={[
              { value: "qwen_realtime", label: "Alibaba Cloud · Qwen3 ASR" },
              { value: "fun_asr_realtime", label: "Alibaba Cloud · Fun-ASR" },
              { value: "openai_realtime", label: "OpenAI Realtime" },
            ]}
            disabled={operationBusy}
            onChange={(value) => onSetCloudBackend(value as CloudBackend)}
          />
          {recognitionProfiles.length > 0 && !apiEditor && (
            <div className="onboarding-profile-select">
              <Select
                label={t("settings.recognition.selectApiProfile")}
                value={selectedProfileId}
                options={recognitionProfiles.map((profile) => ({
                  value: profile.id,
                  label: `${profile.name} · ${profile.credential.configured ? t("settings.apiManagement.configured") : t("settings.apiManagement.notConfigured")}`,
                }))}
                disabled={operationBusy}
                onChange={onSelectProfile}
              />
              <button className="secondary-button" type="button" disabled={operationBusy} onClick={onAddApiProfile}>
                {t("onboarding.recognition.addAnother")}
              </button>
            </div>
          )}
          {!apiEditor && recognitionProfiles.length === 0 && (
            <button className="onboarding-empty-action" type="button" disabled={operationBusy || apiProfiles.loading} onClick={onAddApiProfile}>
              <KeyRound size={19} /><span><strong>{t("onboarding.recognition.addApi")}</strong><small>{t("onboarding.recognition.addApiDescription")}</small></span><ChevronRight size={18} />
            </button>
          )}
          {apiEditor && (
            <ApiProfileEditor
              draft={apiEditor}
              saving={apiProfiles.busy === "create"}
              providers={[provider]}
              purposes={["asr", "shared"]}
              requireCredential
              onChange={onChangeApiEditor}
              onSave={onSaveApiEditor}
              onCancel={onCancelApiEditor}
            />
          )}
          {selectedProfile && !apiEditor && (
            <div className={`onboarding-connection ${testedProfileId === selectedProfile.id ? "ready" : ""}`}>
              <span className="recognition-runtime-dot" />
              <div><strong>{selectedProfile.name}</strong><small>{testedProfileId === selectedProfile.id ? t("onboarding.recognition.connectionReady") : t("onboarding.recognition.testRequired")}</small></div>
              <button className="primary-button" type="button" disabled={operationBusy || !selectedProfile.credential.configured} onClick={onTestAndApplyCloud}>
                {busy ? <RefreshCw className="spin" size={15} /> : <ShieldCheck size={15} />}
                {t("settings.apiManagement.testAsr")}
              </button>
            </div>
          )}
          {apiProfiles.message && <p className="onboarding-feedback" role="status">{apiProfiles.message}</p>}
        </div>
      ) : (
        <div className="onboarding-local-panel">
          <LocalRuntimeStatus capabilities={asrCapabilities} />
          <LocalRecognitionSettings
            draft={draftController.draft}
            disabled={operationBusy}
            capabilities={asrCapabilities}
            asrError={localSettingsError}
            modelStatusLabel={asr.modelStatusLabel}
            computeTypes={validComputeTypes(asrCapabilities, draftController.draft.asr.local.device)}
            selectableModels={asr.selectable}
            onUpdateAsr={asr.updateAsr}
            onUpdateLocalAsr={asr.updateLocalAsr}
          />
          <ModelManagerPanel
            locale={locale}
            disabled={operationBusy}
            installedModels={asr.installed}
            downloadingModels={asr.downloading}
            managedModels={asr.managedModels}
            modelsReady={asr.modelsReady}
            message={asr.message}
            directoryText={asr.modelDirectoryText}
            saveState={draftController.saveState}
            onLoad={asr.loadModels}
            onSetDirectoryText={asr.setModelDirectoryText}
            onUpdateDirectory={asr.updateModelDirectory}
            onChooseDirectory={asr.chooseModelDirectory}
            onDownload={asr.downloadModel}
            onRemove={asr.removeModel}
          />
          {!localReady && <p className="onboarding-feedback">{t("onboarding.recognition.downloadRequired")}</p>}
        </div>
      )}
    </div>
  );
}
