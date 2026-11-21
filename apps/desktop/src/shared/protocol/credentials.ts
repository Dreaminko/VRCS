export interface CredentialStatus {
  configured: boolean;
  stored_configured: boolean;
  environment_override: boolean;
  storage_available: boolean;
  reason_code: string | null;
  source: "environment" | "credential_manager" | "secret_service" | null;
}
