export interface CredentialStatus {
  configured: boolean;
  stored_configured: boolean;
  environment_override: boolean;
  source: "environment" | "credential_manager" | null;
}
