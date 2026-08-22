import { useEffect, useState } from "react";

import type { Page } from "./app-types";
import { CompactEntry } from "./CompactEntry";
import { DesktopShell } from "./DesktopShell";
import { OnboardingEntry } from "./OnboardingEntry";
import { RuntimeWarningDialogs } from "./components/RuntimeNotices";
import { useAppWorkspace } from "./useAppWorkspace";
import type { SettingsCategory } from "../settings/settings-types";

function App() {
  const [page, setPage] = useState<Page>("live");
  const [settingsInitialCategory, setSettingsInitialCategory] = useState<SettingsCategory>("system");
  const workspace = useAppWorkspace({ page, setPage, setSettingsInitialCategory });

  useEffect(() => {
    const preventBrowserContextMenu = (event: MouseEvent) => event.preventDefault();
    document.addEventListener("contextmenu", preventBrowserContextMenu);
    return () => document.removeEventListener("contextmenu", preventBrowserContextMenu);
  }, []);

  let view;
  if (workspace.onboarding.status !== "complete") {
    view = (
      <OnboardingEntry
        onboarding={workspace.onboarding}
        runtime={workspace.runtime}
        capture={workspace.capture}
        settings={workspace.settings}
      />
    );
  } else if (workspace.compact.compact) {
    view = (
      <CompactEntry
        compact={workspace.compact}
        runtime={workspace.runtime}
        capture={workspace.capture}
        settings={workspace.settings}
        selection={workspace.selection}
        learning={workspace.learning}
      />
    );
  } else {
    view = (
      <DesktopShell
        page={page}
        settingsInitialCategory={settingsInitialCategory}
        setPage={setPage}
        setSettingsInitialCategory={setSettingsInitialCategory}
        workspace={workspace}
      />
    );
  }

  return (
    <>
      {view}
      <RuntimeWarningDialogs
        vrchatWarningOpen={workspace.capture.vrchatWarningOpen}
        cudaRuntimeWarningOpen={workspace.runtime.cudaWarning.open}
        onCloseVrchatWarning={workspace.capture.closeVrchatWarning}
        onCloseCudaRuntimeWarning={workspace.runtime.cudaWarning.close}
      />
    </>
  );
}

export default App;
