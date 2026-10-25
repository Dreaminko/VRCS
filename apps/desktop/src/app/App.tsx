import { useEffect, useState } from "react";

import type { Page } from "./app-types";
import { CompactEntry } from "./CompactEntry";
import { DesktopShell } from "./DesktopShell";
import { OnboardingEntry } from "./OnboardingEntry";
import { RuntimeWarningDialogs } from "./components/RuntimeNotices";
import { useAppTheme } from "./useAppTheme";
import { useAppWorkspace } from "./useAppWorkspace";
import type { SettingsCategory } from "../settings/settings-types";
import { useVrDashboardBridge } from "../vr-dashboard/useVrDashboardBridge";
import { useVrControlBridge } from "../vr-controls/useVrControlBridge";
import { PlayerAudioStartDialog } from "../capture/PlayerAudioStartDialog";
import { MemoryCompressionDialog } from "../capture/MemoryCompressionControls";

function App() {
  const [page, setPage] = useState<Page>("live");
  const [settingsInitialCategory, setSettingsInitialCategory] = useState<SettingsCategory>("system");
  useAppTheme();
  const workspace = useAppWorkspace({ page, setPage, setSettingsInitialCategory });
  useVrControlBridge({
    settings: workspace.settings.value,
    getSettings: workspace.settings.getCurrent,
    update: workspace.settings.update,
    available: workspace.runtime.ready && workspace.onboarding.status === "complete",
    requested: workspace.runtime.health?.capture_requested ?? false,
    getRequested: workspace.capture.getRunning,
    running: workspace.runtime.health?.capture_running ?? false,
    busy: workspace.capture.pending,
    setCaptureRunning: workspace.capture.setCaptureRunning,
  });
  useVrDashboardBridge(workspace.settings.value, workspace.settings.update, workspace.providers.catalog.profiles, {
    playerAudio: workspace.runtime.health?.player_audio,
    running: workspace.runtime.health?.capture_requested ?? false,
    available: workspace.runtime.ready && !workspace.capture.pending && workspace.onboarding.status === "complete",
    toggle: workspace.capture.toggleCapture,
  }, workspace.playerDisplay);

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
        playerDisplay={workspace.playerDisplay}
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
      {workspace.capture.playerAudioStart.notice && <PlayerAudioStartDialog
        openedAt={workspace.capture.playerAudioStart.notice.openedAt} onDecision={workspace.capture.playerAudioStart.decide} />}
      {workspace.capture.playerAudioStart.memory.dialog && <MemoryCompressionDialog memory={workspace.capture.playerAudioStart.memory} />}
      <RuntimeWarningDialogs
        vrchatWarningOpen={workspace.capture.vrchatWarningOpen}
        onCloseVrchatWarning={workspace.capture.closeVrchatWarning}
      />
    </>
  );
}

export default App;
