import { useTranslation } from "react-i18next";

import {
  CudaRuntimeDialog,
  VrchatNotRunningDialog,
} from "../../shell/WarningDialogs";

export function RuntimeWarningDialogs({
  vrchatWarningOpen,
  cudaRuntimeWarningOpen,
  onCloseVrchatWarning,
  onCloseCudaRuntimeWarning,
}: {
  vrchatWarningOpen: boolean;
  cudaRuntimeWarningOpen: boolean;
  onCloseVrchatWarning: () => void;
  onCloseCudaRuntimeWarning: () => void;
}) {
  return (
    <>
      {vrchatWarningOpen && (
        <VrchatNotRunningDialog onClose={onCloseVrchatWarning} />
      )}
      {cudaRuntimeWarningOpen && (
        <CudaRuntimeDialog onClose={onCloseCudaRuntimeWarning} />
      )}
    </>
  );
}

export function VrchatMuteToast({
  muted,
  messageKey,
}: {
  muted: boolean;
  messageKey: string;
}) {
  const { t } = useTranslation();

  return (
    <div
      className={`vrchat-mute-toast ${muted ? "muted" : "ready"}`}
      role="status"
    >
      <i aria-hidden="true" />
      <span>{t(messageKey)}</span>
    </div>
  );
}
