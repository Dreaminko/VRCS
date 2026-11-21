import { invoke, isTauri } from "@tauri-apps/api/core";
import type { CoreCapabilities, DesktopCapabilities } from "./capabilities";
import { request } from "./transport";
import type { Health } from "./types";

export const runtimeApi = {
  health: () => request<Health>("/health"),
  capabilities: () => request<CoreCapabilities>("/api/capabilities"),
  platformCapabilities: () => isTauri() ? invoke<DesktopCapabilities>("platform_capabilities") : Promise.resolve(null),
};
