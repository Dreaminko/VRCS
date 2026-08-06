import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { coreApi } from "../../api";
import { localizedError } from "../../app-utils";
import type { AnkiStatus, Settings } from "../../types";
import { createAnkiOptions } from "../settings-derived";
import type { SettingsDraftController } from "./useSettingsDraft";

export function useAnkiSettings({
  active,
  settings,
  draftController,
}: {
  active: boolean;
  settings: Settings;
  draftController: SettingsDraftController;
}) {
  const { t } = useTranslation();
  const [status, setStatus] = useState<AnkiStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [portText, setPortText] = useState(String(settings.anki.port));
  const [portError, setPortError] = useState("");

  useEffect(() => {
    setPortText(String(settings.anki.port));
  }, [settings.anki.port]);

  const getCurrent = draftController.getCurrent;

  const loadStatus = useCallback(async () => {
    if (!getCurrent().anki.enabled) {
      setStatus(null);
      setMessage("");
      setBusy(false);
      return;
    }
    setBusy(true);
    setMessage("");
    try {
      const next = await coreApi.ankiStatus();
      if (!getCurrent().anki.enabled) return;
      setStatus(next);
      setMessage(t(`apiStatus.${next.status_code}`, {
        ...next.params,
        defaultValue: next.detail,
      }));
    } catch (reason) {
      if (!getCurrent().anki.enabled) return;
      setStatus(null);
      setMessage(localizedError(reason, t, "errors.anki.status"));
    } finally {
      setBusy(false);
    }
  }, [getCurrent, t]);

  useEffect(() => {
    if (active && draftController.draft.anki.enabled) void loadStatus();
    else if (!draftController.draft.anki.enabled) {
      setStatus(null);
      setMessage("");
      setPortError("");
      setBusy(false);
    }
  }, [active, draftController.draft.anki.enabled, loadStatus]);

  const update = <K extends keyof Settings["anki"]>(
    key: K,
    value: Settings["anki"][K],
  ) => {
    draftController.applySettings(
      (current) => ({ ...current, anki: { ...current.anki, [key]: value } }),
      () => {
        if (getCurrent().anki.enabled) void loadStatus();
        else {
          setStatus(null);
          setMessage("");
        }
      },
    );
  };

  const commitPort = () => {
    const port = Number(portText);
    if (!Number.isInteger(port) || port < 1 || port > 65_535) {
      setPortError(t("settings.anki.invalidPort"));
      return;
    }
    setPortError("");
    if (port !== draftController.getCurrent().anki.port) update("port", port);
  };

  const options = useMemo(
    () => createAnkiOptions(status, draftController.draft.anki),
    [draftController.draft.anki, status],
  );

  return {
    status,
    busy,
    message,
    portText,
    portError,
    setPortText,
    loadStatus,
    update,
    commitPort,
    ...options,
  };
}
