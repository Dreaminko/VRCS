import { request } from "../core-client/transport";
import type { ModelDownloadSource } from "../settings/types";

export interface DownloadSourceProbe {
  official_latency_ms: number | null;
  mirror_latency_ms: number | null;
  recommended_source: Exclude<ModelDownloadSource, "auto"> | null;
}

interface DownloadSourceStatus {
  configured_source: ModelDownloadSource;
  active_source: Exclude<ModelDownloadSource, "auto"> | null;
  probe: DownloadSourceProbe | null;
}

export function getDownloadSources(signal: AbortSignal): Promise<DownloadSourceStatus> {
  return request("/api/downloads/sources", { signal });
}

export function probeDownloadSources(signal: AbortSignal, refresh = false): Promise<DownloadSourceProbe> {
  return request(`/api/downloads/probe?refresh=${refresh}`, { method: "POST", signal });
}
