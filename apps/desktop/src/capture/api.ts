import { request } from "../core-client/transport";
import type { AudioDevice } from "./types";

export const captureApi = {
  devices: () => request<AudioDevice[]>("/api/audio/devices"),
  start: () => request<{
    running: boolean;
    device: AudioDevice | null;
    microphone_device: AudioDevice | null;
  }>("/api/capture/start", {
    method: "POST",
    timeoutMs: 150_000,
    body: JSON.stringify({}),
  }),
  stop: () => request<{ running: boolean }>("/api/capture/stop", { method: "POST", timeoutMs: 600_000 }),
  startNetworkCapture: () => request<{ running: boolean; network_capture_requested: boolean }>(
    "/api/capture/network/start",
    { method: "POST", timeoutMs: 30_000 },
  ),
  stopNetworkCapture: () => request<{ running: boolean; network_capture_requested: boolean; capture_requested: boolean }>(
    "/api/capture/network/stop",
    { method: "POST", timeoutMs: 30_000 },
  ),
  startMicrophoneTest: () => request<{ running: boolean; device: AudioDevice }>(
    "/api/audio/microphone-test/start",
    { method: "POST" },
  ),
  stopMicrophoneTest: () => request<{ running: boolean }>(
    "/api/audio/microphone-test/stop",
    { method: "POST" },
  ),
};
