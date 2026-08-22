import { invoke, isTauri } from "@tauri-apps/api/core";

import { resetSettingsRevision } from "../settings/api";
import { initializeCoreTransport } from "./transport";

export interface CoreStartup {
  state: "starting" | "ready" | "failed";
  error: string | null;
}

export async function initializeCoreApi(): Promise<void> {
  resetSettingsRevision();
  await initializeCoreTransport();
}

export async function coreStartup(): Promise<CoreStartup> {
  if (!isTauri()) return { state: "ready", error: null };
  return invoke<CoreStartup>("core_startup");
}

export async function retryCore(): Promise<void> {
  if (!isTauri()) return;
  await invoke("retry_core");
  resetSettingsRevision();
}
