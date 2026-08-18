import { X } from "lucide-react";
import { useTranslation } from "react-i18next";

export function ErrorBanner({
  error,
  retryable,
  onRetry,
  onClose,
}: {
  error: string;
  retryable: boolean;
  onRetry: () => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();

  return (
    <div className="error-banner" role="alert">
      <span>{error}</span>
      {retryable && (
        <button type="button" onClick={onRetry}>
          {t("common.retry")}
        </button>
      )}
      <button
        type="button"
        aria-label={t("common.closeError")}
        onClick={onClose}
      >
        <X size={18} />
      </button>
    </div>
  );
}
