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
import { useVrPlayerDisplayBridge } from "../vr-controls/useVrPlayerDisplayBridge";
import { PlayerAudioStartDialog } from "../capture/PlayerAudioStartDialog";
import { MemoryCompressionDialog } from "../capture/MemoryCompressionControls";
import { PlatformCapabilitiesContext } from "../core-client/capabilities";

function VrPlayerDisplayBridge({ epoch, workspace }: { epoch: number; workspace: ReturnType<typeof useAppWorkspace> }) {
  useVrPlayerDisplayBridge(epoch, workspace);
  return null;
}

function App() {
  const [page, setPage] = useState<Page>("live");
  const [settingsInitialCategory, setSettingsInitialCategory] = useState<SettingsCategory>("system");
  useAppTheme();
  const workspace = useAppWorkspace({ page, setPage, setSettingsInitialCategory });
  const platformOs = workspace.runtime.capabilities.desktop?.os ?? workspace.runtime.capabilities.core?.os;
  const vrSceneCapture = workspace.runtime.capabilities.desktop?.vr.scene_capture.supported === true;
  const networkAvailable = workspace.runtime.ready && workspace.onboarding.status === "complete"
    && workspace.runtime.capabilities.desktop?.vr.controller_input.supported === true
    && workspace.capture.networkCaptureVisible;
  const controlEpoch = useVrControlBridge({
    os: platformOs,
    settings: workspace.settings.value,
    getSettings: workspace.settings.getCurrent,
    update: workspace.settings.update,
    available: workspace.runtime.ready && workspace.onboarding.status === "complete"
      && workspace.runtime.capabilities.desktop?.vr.controller_input.supported === true,
    sceneCapture: vrSceneCapture,
    requested: workspace.runtime.health?.capture_requested,
    getRequested: workspace.capture.getRunning,
    running: workspace.runtime.health?.capture_running ?? false,
    busy: workspace.capture.pending,
    setCaptureRunning: workspace.capture.setCaptureRunning,
    networkVisible: workspace.capture.networkCaptureVisible,
    networkRequested: workspace.runtime.health?.network_capture_requested,
    getNetworkRequested: workspace.capture.getNetworkRunning,
    networkRunning: workspace.runtime.health?.network_capture_running ?? false,
    networkBusy: workspace.capture.networkPending,
    setNetworkCaptureRunning: workspace.capture.setNetworkCaptureRunning,
    playerAudioStart: workspace.capture.playerAudioStart,
    playerAudioStatus: workspace.runtime.health?.network_capture ?? workspace.runtime.health?.player_audio,
    chatboxStatus: workspace.runtime.health?.vrchat_chatbox,
  });
  useVrDashboardBridge(workspace.settings.value, workspace.settings.update, workspace.providers.catalog.profiles, {
    playerAudio: workspace.runtime.health?.network_capture ?? workspace.runtime.health?.player_audio,
    running: workspace.runtime.health?.capture_requested ?? false,
    available: workspace.runtime.ready && !workspace.capture.pending && workspace.onboarding.status === "complete"
      && workspace.runtime.capabilities.desktop?.vr.controller_input.supported === true,
    toggle: workspace.capture.toggleCapture,
    networkVisible: workspace.capture.networkCaptureVisible,
    networkRunning: workspace.runtime.health?.network_capture_requested ?? false,
    networkAvailable: networkAvailable && !workspace.capture.networkPending && (Boolean(workspace.runtime.health?.network_capture_requested) || workspace.capture.networkAvailable),
  }, workspace.playerDisplay, vrSceneCapture);

  useEffect(() => {
    const preventBrowserContextMenu = (event: MouseEvent) => event.preventDefault();
    document.addEventListener("contextmenu", preventBrowserContextMenu);
    return () => document.removeEventListener("contextmenu", preventBrowserContextMenu);
  }, []);

  const onboardingComplete = workspace.onboarding.status === "complete";
  const compactActive = onboardingComplete && workspace.compact.compact;

  return (
    <PlatformCapabilitiesContext.Provider value={workspace.runtime.capabilities}>
      {controlEpoch > 0 && <VrPlayerDisplayBridge epoch={controlEpoch} workspace={workspace} />}
      {!onboardingComplete && (
        <OnboardingEntry
          onboarding={workspace.onboarding}
          runtime={workspace.runtime}
          capture={workspace.capture}
          settings={workspace.settings}
        />
      )}
      {onboardingComplete && !compactActive && (
        <DesktopShell
          page={page}
          settingsInitialCategory={settingsInitialCategory}
          setPage={setPage}
          setSettingsInitialCategory={setSettingsInitialCategory}
          workspace={workspace}
        />
      )}
      {compactActive && (
        <CompactEntry
          playerDisplay={workspace.playerDisplay}
          compact={workspace.compact}
          runtime={workspace.runtime}
          capture={workspace.capture}
          settings={workspace.settings}
          selection={workspace.selection}
          learning={workspace.learning}
          chatbox={workspace.chatbox}
          providers={workspace.providers}
        />
      )}
      {workspace.capture.playerAudioStart.notice && <PlayerAudioStartDialog
        openedAt={workspace.capture.playerAudioStart.notice.openedAt} os={platformOs} onDecision={workspace.capture.playerAudioStart.decide} />}
      {workspace.capture.playerAudioStart.memory.dialog && <MemoryCompressionDialog memory={workspace.capture.playerAudioStart.memory} />}
      <RuntimeWarningDialogs
        vrchatWarningOpen={workspace.capture.vrchatWarningOpen}
        onCloseVrchatWarning={workspace.capture.closeVrchatWarning}
      />
    </PlatformCapabilitiesContext.Provider>
  );
}

export default App;
