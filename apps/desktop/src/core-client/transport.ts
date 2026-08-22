import { invoke, isTauri } from "@tauri-apps/api/core";

import { apiErrorFromResponse } from "../api-error";

interface CoreConnection {
  httpUrl: string;
  wsUrl: string;
  token: string;
}

export type CoreRequestInit = RequestInit & { timeoutMs?: number };

let connection: CoreConnection = {
  httpUrl: "http://127.0.0.1:8766",
  wsUrl: "ws://127.0.0.1:8766/ws",
  token: import.meta.env.VITE_VRCS_SESSION_TOKEN ?? "",
};

const DEFAULT_REQUEST_TIMEOUT_MS = 15_000;

function requestHeaders(initial?: HeadersInit): Headers {
  const headers = new Headers(initial);
  if (!headers.has("Content-Type")) headers.set("Content-Type", "application/json");
  if (connection.token) headers.set("Authorization", `Bearer ${connection.token}`);
  return headers;
}

export async function initializeCoreTransport(): Promise<void> {
  if (isTauri()) {
    connection = await invoke<CoreConnection>("core_connection");
  }
}

export async function timedCoreFetch(
  path: string,
  init?: CoreRequestInit,
): Promise<Response> {
  const { timeoutMs = DEFAULT_REQUEST_TIMEOUT_MS, ...fetchInit } = init ?? {};
  const controller = new AbortController();
  const abortFromCaller = () => controller.abort(fetchInit.signal?.reason);
  if (fetchInit.signal?.aborted) abortFromCaller();
  else fetchInit.signal?.addEventListener("abort", abortFromCaller, { once: true });
  const timer = window.setTimeout(
    () => controller.abort(new DOMException("Request timed out", "TimeoutError")),
    timeoutMs,
  );
  try {
    return await fetch(`${connection.httpUrl}${path}`, {
      ...fetchInit,
      headers: requestHeaders(fetchInit.headers),
      signal: controller.signal,
    });
  } finally {
    window.clearTimeout(timer);
    fetchInit.signal?.removeEventListener("abort", abortFromCaller);
  }
}

export function rawCoreFetch(path: string, init?: RequestInit): Promise<Response> {
  return fetch(`${connection.httpUrl}${path}`, {
    ...init,
    headers: requestHeaders(init?.headers),
  });
}

export async function request<T>(path: string, init?: CoreRequestInit): Promise<T> {
  const response = await timedCoreFetch(path, init);
  if (!response.ok) throw await apiErrorFromResponse(response);
  return (await response.json()) as T;
}

export function coreWebSocketUrl(): string {
  const url = new URL(connection.wsUrl);
  if (connection.token) url.searchParams.set("token", connection.token);
  return url.toString();
}
