import { Channel, invoke, isTauri } from "@tauri-apps/api/core";

const RELEASE_PAGE_URL = "https://github.com/Dreaminko/VRCS/releases/latest";

export interface AppBuildInfo {
  version: string;
  variant: "standard";
  os?: string;
  arch?: string;
  packageChannel?: "development" | "installer" | "manual" | "system" | "deb" | "rpm" | "appimage";
  updaterAvailable: boolean;
  canInstall?: boolean;
}

export interface AppUpdateMetadata {
  version: string;
  currentVersion: string;
  notes: string | null;
  channel: "stable" | "nightly";
  releaseUrl?: string | null;
}

export type UpdateDownloadEvent =
  | { event: "started"; data: { contentLength: number | null } }
  | { event: "progress"; data: { chunkLength: number } }
  | { event: "finished" };

export async function loadAppBuildInfo(): Promise<AppBuildInfo> {
  if (!isTauri()) {
    return { version: "development", variant: "standard", updaterAvailable: false, canInstall: false };
  }
  return invoke<AppBuildInfo>("app_build_info");
}

export async function openReleasePage(url?: string | null): Promise<void> {
  const target = url || RELEASE_PAGE_URL;
  if (isTauri()) {
    await invoke("open_project_link", { target });
    return;
  }
  window.open(target, "_blank", "noopener,noreferrer");
}

export async function checkForAppUpdate(): Promise<AppUpdateMetadata | null> {
  if (!isTauri()) return null;
  return invoke<AppUpdateMetadata | null>("check_for_update");
}

export async function downloadAndInstallAppUpdate(
  version: string,
  onEvent: (event: UpdateDownloadEvent) => void,
): Promise<void> {
  if (!isTauri()) return;
  const channel = new Channel<UpdateDownloadEvent>();
  channel.onmessage = onEvent;
  await invoke("download_and_install_update", { version, onEvent: channel });
}

export function updaterErrorCode(reason: unknown): string {
  return typeof reason === "string" && reason.startsWith("update.")
    ? reason.slice("update.".length)
    : "failed";
}
