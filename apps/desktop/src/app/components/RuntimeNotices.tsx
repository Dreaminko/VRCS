import { useTranslation } from "react-i18next";

import { VrchatNotRunningDialog } from "../../shell/WarningDialogs";

export function RuntimeWarningDialogs({
  vrchatWarningOpen,
  onCloseVrchatWarning,
}: {
  vrchatWarningOpen: boolean;
  onCloseVrchatWarning: () => void;
}) {
  return (
    <>
      {vrchatWarningOpen && (
        <VrchatNotRunningDialog onClose={onCloseVrchatWarning} />
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
