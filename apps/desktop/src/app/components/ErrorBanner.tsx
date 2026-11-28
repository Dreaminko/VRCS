import { ArrowRight, X } from "lucide-react";
import { useTranslation } from "react-i18next";

export function ErrorBanner({
  error,
  retryable,
  onRetry,
  actionLabel,
  onAction,
  onClose,
}: {
  error: string;
  retryable: boolean;
  onRetry: () => void;
  actionLabel?: string;
  onAction?: () => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();

  return (
    <div className="error-banner" role="alert">
      <span>{error}</span>
      <div className="error-banner-actions">
        {actionLabel && onAction && (
          <button
            className="error-banner-action"
            type="button"
            onClick={onAction}
          >
            {actionLabel}
            <ArrowRight size={14} aria-hidden="true" />
          </button>
        )}
        {retryable && (
          <button className="error-banner-action" type="button" onClick={onRetry}>
            {t("common.retry")}
          </button>
        )}
        <button
          className="error-banner-close"
          type="button"
          aria-label={t("common.closeError")}
          onClick={onClose}
        >
          <X size={18} />
        </button>
      </div>
    </div>
  );
}
