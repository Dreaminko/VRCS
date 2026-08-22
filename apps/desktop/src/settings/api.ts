import { apiErrorFromResponse } from "../api-error";
import { timedCoreFetch } from "../core-client/transport";
import type { Settings } from "./types";

interface ConfigRevision {
  token: string;
  epoch: string;
  counter: number;
}

let configRevision: ConfigRevision | null = null;

export function resetSettingsRevision(): void {
  configRevision = null;
}

function parseConfigRevision(value: string | null): ConfigRevision | null {
  if (value === null) return null;
  const separator = value.lastIndexOf(":");
  if (separator <= 0) return null;
  const epoch = value.slice(0, separator);
  const counter = Number.parseInt(value.slice(separator + 1), 10);
  if (!Number.isSafeInteger(counter) || counter < 0) return null;
  return { token: value, epoch, counter };
}

async function settingsRequest(
  init?: RequestInit,
  retryStaleResponse = true,
): Promise<Settings> {
  const headers = new Headers(init?.headers);
  if (init?.method === "PUT" && configRevision !== null) {
    headers.set("X-VRCS-Config-Revision", configRevision.token);
  }
  const response = await timedCoreFetch("/api/settings", { ...init, headers });
  if (!response.ok) throw await apiErrorFromResponse(response);

  const responseRevision = parseConfigRevision(
    response.headers.get("X-VRCS-Config-Revision"),
  );
  if (
    responseRevision !== null
    && configRevision !== null
    && responseRevision.epoch === configRevision.epoch
    && responseRevision.counter < configRevision.counter
  ) {
    if (retryStaleResponse) {
      return settingsRequest(init?.signal ? { signal: init.signal } : undefined, false);
    }
    throw new Error("The Core returned an outdated settings revision");
  }
  if (responseRevision !== null && (
    configRevision === null
    || responseRevision.epoch !== configRevision.epoch
    || responseRevision.counter >= configRevision.counter
  )) {
    configRevision = responseRevision;
  }
  return (await response.json()) as Settings;
}

export const settingsApi = {
  settings: () => settingsRequest(),
  saveSettings: (settings: Settings) => settingsRequest({
    method: "PUT",
    body: JSON.stringify(settings),
  }),
};
