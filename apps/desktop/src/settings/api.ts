import { apiErrorFromResponse } from "../core-client/api-error";
import { timedCoreFetch } from "../core-client/transport";
import type { Settings } from "./types";

interface ConfigRevision {
  token: string;
  epoch: string;
  counter: number;
}

let configRevision: ConfigRevision | null = null;
let revisionGeneration = 0;

export function resetSettingsRevision(): void {
  revisionGeneration += 1;
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

function isStaleRevision(responseRevision: ConfigRevision | null): boolean {
  return responseRevision !== null
    && configRevision !== null
    && responseRevision.epoch === configRevision.epoch
    && responseRevision.counter < configRevision.counter;
}

function commitConfigRevision(
  responseRevision: ConfigRevision | null,
  generation: number,
): void {
  if (generation !== revisionGeneration || responseRevision === null) return;
  if (
    configRevision === null
    || responseRevision.epoch !== configRevision.epoch
    || responseRevision.counter >= configRevision.counter
  ) {
    configRevision = responseRevision;
  }
}

async function fetchSettings(
  signal?: AbortSignal,
  retryStaleResponse = true,
): Promise<Settings> {
  const generation = revisionGeneration;
  const response = await timedCoreFetch("/api/settings", { signal });
  if (!response.ok) throw await apiErrorFromResponse(response);

  const responseRevision = parseConfigRevision(
    response.headers.get("X-VRCS-Config-Revision"),
  );
  if (generation === revisionGeneration && isStaleRevision(responseRevision)) {
    if (retryStaleResponse) return fetchSettings(signal, false);
    throw new Error("The Core returned an outdated settings revision");
  }
  commitConfigRevision(responseRevision, generation);
  return (await response.json()) as Settings;
}

async function putSettings(settings: Settings): Promise<Settings> {
  const generation = revisionGeneration;
  const headers = new Headers();
  if (configRevision !== null) {
    headers.set("X-VRCS-Config-Revision", configRevision.token);
  }
  const response = await timedCoreFetch("/api/settings", {
    method: "PUT",
    headers,
    body: JSON.stringify(settings),
    timeoutMs: 150_000,
  });
  if (!response.ok) throw await apiErrorFromResponse(response);

  const responseRevision = parseConfigRevision(
    response.headers.get("X-VRCS-Config-Revision"),
  );
  if (generation === revisionGeneration && isStaleRevision(responseRevision)) {
    throw Object.assign(
      new Error("The Core returned an outdated settings revision"),
      { status: 409 },
    );
  }
  commitConfigRevision(responseRevision, generation);
  return (await response.json()) as Settings;
}

export const settingsApi = {
  settings: (signal?: AbortSignal) => fetchSettings(signal),
  saveSettings: (settings: Settings) => putSettings(settings),
};
