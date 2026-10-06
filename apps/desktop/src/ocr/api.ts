import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { DesktopOcrStatus } from "./status";

export async function getDesktopOcrStatus(): Promise<DesktopOcrStatus> {
  if (!isTauri()) return { scan_id: 0, revision: 0, state: "disabled", blocks: [], error: null, shortcut_error: null, timed_out: false };
  return invoke("desktop_ocr_status");
}

export async function scanDesktopOcr(): Promise<void> {
  if (isTauri()) await invoke("desktop_ocr_scan");
}

export async function closeDesktopOcr(): Promise<void> {
  if (isTauri()) await invoke("desktop_ocr_close");
}

export async function listenDesktopOcrStatus(onStatus: (status: DesktopOcrStatus) => void): Promise<UnlistenFn> {
  if (!isTauri()) return () => undefined;
  return listen<DesktopOcrStatus>("desktop-ocr-status-changed", (event) => onStatus(event.payload));
}
