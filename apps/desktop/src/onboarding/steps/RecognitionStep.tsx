import { Check, ChevronRight, Cloud, HardDrive, KeyRound, RefreshCw, ShieldCheck } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { ApiProfileEditorDraft } from "../../api-profile-draft";
import { ApiProfileEditor } from "../../settings/api/ApiProfileEditor";
import type { useAsrModels } from "../../settings/hooks/useAsrModels";
import type { SettingsDraftController } from "../../settings/hooks/useSettingsDraft";
import { LocalRecognitionSettings, LocalRuntimeStatus } from "../../settings/recognition/LocalRecognitionSettings";
import { ModelManagerPanel } from "../../settings/recognition/ModelManagerPanel";
import { Select } from "../../settings/SettingsControls";
import { validComputeTypes } from "../../settings/settings-validation";
import type { useApiProfiles } from "../../settings/useApiProfiles";
import type {
  ApiProfileView,
  AsrCapabilities,
  ProviderServiceDefinition,
} from "../../providers/types";
import type { RecognitionMode } from "../onboarding-types";

type AsrModelsController = ReturnType<typeof useAsrModels>;
type ApiProfilesController = ReturnType<typeof useApiProfiles>;

export function RecognitionStep({
  recognitionMode,
  operationBusy,
  recognitionProfiles,
  recognitionServices,
  selectedProfileId,
  selectedServiceId,
  testedSelectionId,
  selectedProfile,
  selectedService,
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
  onSelectProfile,
  onSelectService,
  onAddApiProfile,
  onChangeApiEditor,
  onSaveApiEditor,
  onCancelApiEditor,
  onTestAndApplyCloud,
}: {
  recognitionMode: RecognitionMode;
  operationBusy: boolean;
  recognitionProfiles: ApiProfileView[];
  recognitionServices: ProviderServiceDefinition[];
  selectedProfileId: string;
  selectedServiceId: string;
  testedSelectionId: string;
  selectedProfile: ApiProfileView | undefined;
  selectedService: ProviderServiceDefinition | undefined;
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
  onSelectProfile: (profileId: string) => void;
  onSelectService: (serviceId: string) => void;
  onAddApiProfile: () => void;
  onChangeApiEditor: (draft: ApiProfileEditorDraft) => void;
  onSaveApiEditor: () => void;
  onCancelApiEditor: () => void;
  onTestAndApplyCloud: () => void;
}) {
  const { t } = useTranslation();
  const selectionId = selectedProfile && selectedService
    ? `${selectedProfile.id}:${selectedService.id}`
    : "";
  const connectionReady = Boolean(selectionId && testedSelectionId === selectionId);

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
          {selectedProfile && recognitionServices.length > 0 && !apiEditor && (
            <Select
              label={t("settings.recognition.cloudService")}
              value={selectedServiceId}
              options={recognitionServices.map((service) => ({
                value: service.id,
                label: service.display_name,
                description: service.recognition_transport
                  ? t(`settings.recognition.transports.${service.recognition_transport}`)
                  : undefined,
              }))}
              disabled={operationBusy}
              onChange={onSelectService}
            />
          )}
          {!apiEditor && recognitionProfiles.length === 0 && (
            <button className="onboarding-empty-action" type="button" disabled={operationBusy || apiProfiles.loading || apiProfiles.providerDefinitions.length === 0} onClick={onAddApiProfile}>
              <KeyRound size={19} /><span><strong>{t("onboarding.recognition.addApi")}</strong><small>{t("onboarding.recognition.addApiDescription")}</small></span><ChevronRight size={18} />
            </button>
          )}
          {apiEditor && (
            <ApiProfileEditor
              draft={apiEditor}
              saving={apiProfiles.busy === "create"}
              providerDefinitions={apiProfiles.providerDefinitions}
              requireCredential
              requiredCapability="speech_to_text"
              onChange={onChangeApiEditor}
              onSave={onSaveApiEditor}
              onCancel={onCancelApiEditor}
            />
          )}
          {selectedProfile && selectedService && !apiEditor && (
            <>
              {selectedService.recognition_transport === "segmented_upload" && (
                <div className="cloud-transport-hint" role="note">
                  <strong>{t("settings.recognition.segmentedUpload.title")}</strong>
                  <small>{t("settings.recognition.segmentedUpload.description")}</small>
                  {!selectedService.partial_results && <small>{t("settings.recognition.segmentedUpload.noPartial")}</small>}
                </div>
              )}
              <div className={`onboarding-connection ${connectionReady ? "ready" : ""}`}>
                <span className="recognition-runtime-dot" />
                <div><strong>{selectedService.display_name}</strong><small>{connectionReady ? t("onboarding.recognition.connectionReady") : t("onboarding.recognition.testRequired")}</small></div>
                <button className="primary-button" type="button" disabled={operationBusy || !selectedProfile.credential.configured} onClick={onTestAndApplyCloud}>
                  {busy ? <RefreshCw className="spin" size={15} /> : <ShieldCheck size={15} />}
                  {t("settings.apiManagement.testAsr")}
                </button>
              </div>
            </>
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
