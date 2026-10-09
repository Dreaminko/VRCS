import { Trash2 } from "lucide-react";
import type { RefObject } from "react";
import { useTranslation } from "react-i18next";
import { SettingsDialog } from "./SettingsDialog";

export function ModelDeleteDialog({ name, removing, returnFocusRef, onClose, onConfirm }: {
  name: string;
  removing: boolean;
  returnFocusRef: RefObject<HTMLButtonElement | null>;
  onClose: () => void;
  onConfirm: () => Promise<void>;
}) {
  const { t } = useTranslation();
  return <SettingsDialog
    label={t("settings.recognition.deleteModel", { name })}
    saving={removing}
    returnFocusRef={returnFocusRef}
    className="model-delete-dialog"
    onClose={onClose}
  >
    <div className="api-profile-editor">
      <div className="api-profile-editor-heading"><strong>{t("common.delete")}</strong></div>
      <div className="api-profile-editor-content">
        <p className="model-delete-dialog-description">{t("settings.recognition.confirmDelete", { name })}</p>
      </div>
      <div className="api-profile-editor-actions">
        <div />
        <div className="settings-inline-actions">
          <button className="secondary-button" type="button" autoFocus disabled={removing} onClick={onClose}>{t("common.cancel")}</button>
          <button className="secondary-button api-danger-button" type="button" disabled={removing} onClick={() => void onConfirm()}><Trash2 size={16} />{t(removing ? "common.loading" : "common.delete")}</button>
        </div>
      </div>
    </div>
  </SettingsDialog>;
}
