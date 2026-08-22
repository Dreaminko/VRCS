import { request } from "../core-client/transport";
import type { CredentialStatus } from "../shared/protocol/credentials";
import type { ExternalApiRuntimeStatus, VrcxRuntimeStatus } from "./types";

export const integrationsApi = {
  externalApiTokenStatus: () => request<CredentialStatus>("/api/external-api/token"),
  externalApiRuntimeStatus: () => request<ExternalApiRuntimeStatus>("/api/external-api/status"),
  saveExternalApiToken: (token: string) => request<CredentialStatus>(
    "/api/external-api/token",
    { method: "PUT", body: JSON.stringify({ token }) },
  ),
  deleteExternalApiToken: () => request<CredentialStatus>(
    "/api/external-api/token",
    { method: "DELETE" },
  ),
  vrcxTokenStatus: () => request<CredentialStatus>("/api/vrcx/token"),
  vrcxRuntimeStatus: () => request<VrcxRuntimeStatus>("/api/vrcx/status"),
  saveVrcxToken: (token: string) => request<CredentialStatus>(
    "/api/vrcx/token",
    { method: "PUT", body: JSON.stringify({ token }) },
  ),
  deleteVrcxToken: () => request<CredentialStatus>(
    "/api/vrcx/token",
    { method: "DELETE" },
  ),
  testVrcx: () => request<VrcxRuntimeStatus>("/api/vrcx/test", { method: "POST" }),
  testOsc: () => request<{ queued: boolean }>("/api/osc/test", { method: "POST" }),
};
