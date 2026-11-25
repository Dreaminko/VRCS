import { Channel, invoke, isTauri } from "@tauri-apps/api/core";

import { resolveUpdateErrorKey, stripPlatformNotesSection } from "./update-notes.ts";

const RELEASE_PAGE_URL = "https://github.com/Dreaminko/VRCS/releases/latest";
const RELEASE_API_BASE = "https://api.github.com/repos/Dreaminko/VRCS/releases/tags";
const RELEASE_TAG_PATTERN = /^[0-9A-Za-z._-]+$/;

export interface AppBuildInfo {
  version: string;
  variant: "standard";
  os?: string;
  arch?: string;
  packageChannel?: "development" | "installer" | "manual" | "system" | "deb" | "rpm" | "appimage";
  updaterAvailable: boolean;
  canInstall: boolean;
}

export interface AppUpdateMetadata {
  version: string;
  currentVersion: string;
  notes: string | null;
  publishedAt?: string | null;
  channel: "stable" | "nightly";
  releaseTag?: string | null;
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

export async function checkForAppUpdate(background = false): Promise<AppUpdateMetadata | null> {
  if (!isTauri()) return null;
  return invoke<AppUpdateMetadata | null>("check_for_update", { background });
}

export async function fetchGitHubReleaseNotes(
  tag: string,
  signal?: AbortSignal,
): Promise<string | null> {
  const trimmedTag = tag.trim();
  if (!RELEASE_TAG_PATTERN.test(trimmedTag)) return null;
  try {
    const timeoutSignal = AbortSignal.timeout(8_000);
    const requestSignal = signal ? AbortSignal.any([signal, timeoutSignal]) : timeoutSignal;
    const response = await fetch(`${RELEASE_API_BASE}/${encodeURIComponent(trimmedTag)}`, {
      headers: { Accept: "application/vnd.github+json" },
      signal: requestSignal,
    });
    if (!response.ok) return null;
    const payload: unknown = await response.json();
    if (!payload || typeof payload !== "object") return null;
    const body = (payload as { body?: unknown }).body;
    if (typeof body !== "string") return null;
    const stripped = stripPlatformNotesSection(body);
    return stripped || null;
  } catch {
    return null;
  }
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
  if (typeof reason === "string" && reason.startsWith("update.")) {
    return resolveUpdateErrorKey(reason.slice("update.".length));
  }
  return "failed";
}
