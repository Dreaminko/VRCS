import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { coreApi } from "../../api";
import type { CredentialStatus } from "../../types";

export type CloudCredentialProvider = "qwen" | "openai";

const emptyCredentials = (): Record<CloudCredentialProvider, CredentialStatus> => ({
  qwen: { configured: false, source: null },
  openai: { configured: false, source: null },
});

export function useAsrCredentials(provider: CloudCredentialProvider) {
  const { t } = useTranslation();
  const [credentials, setCredentials] = useState<Record<CloudCredentialProvider, CredentialStatus> | null>(null);
  const [apiKey, setApiKey] = useState("");
  const [message, setMessage] = useState("");

  useEffect(() => {
    void coreApi.asrCredentials().then(setCredentials).catch(() => setCredentials(null));
  }, []);

  useEffect(() => {
    setApiKey("");
    setMessage("");
  }, [provider]);

  const save = async () => {
    if (!apiKey.trim()) return;
    try {
      const status = await coreApi.saveAsrCredential(provider, apiKey);
      setCredentials((current) => ({ ...(current ?? emptyCredentials()), [provider]: status }));
      setApiKey("");
      setMessage(t("settings.recognition.credentialSaved"));
    } catch (reason) {
      setMessage(reason instanceof Error ? reason.message : String(reason));
    }
  };

  const test = async () => {
    try {
      await coreApi.testAsrCredential(provider);
      setMessage(t("settings.recognition.connectionSucceeded"));
    } catch (reason) {
      setMessage(reason instanceof Error ? reason.message : String(reason));
    }
  };

  const remove = async () => {
    try {
      const status = await coreApi.deleteAsrCredential(provider);
      setCredentials((current) => ({ ...(current ?? emptyCredentials()), [provider]: status }));
      setMessage("");
    } catch (reason) {
      setMessage(reason instanceof Error ? reason.message : String(reason));
    }
  };

  return {
    status: credentials?.[provider] ?? null,
    apiKey,
    message,
    setApiKey,
    save,
    test,
    remove,
  };
}
