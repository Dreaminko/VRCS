import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { localizedError } from "../../app/app-utils";
import type { DictionarySource } from "../../dictionary/types";

export function useDictionaryActions({
  locale,
  onImport,
  onDelete,
}: {
  locale: string;
  onImport: (file: File, onProgress?: (progress: number) => void) => Promise<DictionarySource>;
  onDelete: (id: number) => Promise<void>;
}) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [messageError, setMessageError] = useState(false);
  const [progress, setProgress] = useState<number | null>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const choose = async (file?: File) => {
    if (!file) return;
    setBusy(true);
    setProgress(0);
    setMessageError(false);
    setMessage(t("settings.dictionary.importing", { file: file.name }));
    try {
      const imported = await onImport(file, setProgress);
      setMessage(t("settings.dictionary.imported", {
        title: imported.title,
        count: imported.entry_count,
        formatted: new Intl.NumberFormat(locale).format(imported.entry_count),
      }));
    } catch (reason) {
      setMessageError(true);
      setMessage(localizedError(reason, t, "errors.dictionary.import"));
    } finally {
      setBusy(false);
      setProgress(null);
      if (fileInputRef.current) fileInputRef.current.value = "";
    }
  };

  const remove = async (dictionary: DictionarySource) => {
    setBusy(true);
    setMessageError(false);
    try {
      await onDelete(dictionary.id);
      setMessage(t("settings.dictionary.removed", { title: dictionary.title }));
    } catch (reason) {
      setMessageError(true);
      setMessage(localizedError(reason, t, "errors.dictionary.remove"));
    } finally {
      setBusy(false);
    }
  };

  return {
    busy,
    message,
    messageError,
    progress,
    fileInputRef,
    choose,
    remove,
  };
}
