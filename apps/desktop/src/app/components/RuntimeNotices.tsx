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
