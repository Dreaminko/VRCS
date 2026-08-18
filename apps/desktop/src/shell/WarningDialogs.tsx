import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { TriangleAlert } from "lucide-react";

export function VrchatNotRunningDialog({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  return (
    <WarningDialog
      id="vrchat-warning"
      title={t("warnings.vrchat.title")}
      description={t("warnings.vrchat.description")}
      onClose={onClose}
    />
  );
}

export function CudaRuntimeDialog({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  return (
    <WarningDialog
      id="cuda-runtime-warning"
      title={t("warnings.cuda.title")}
      description={t("warnings.cuda.description")}
      onClose={onClose}
    />
  );
}

function WarningDialog({ id, title, description, onClose }: {
  id: string;
  title: string;
  description: string;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const confirmRef = useRef<HTMLButtonElement>(null);
  const onCloseRef = useRef(onClose);

  useEffect(() => {
    onCloseRef.current = onClose;
  }, [onClose]);

  useEffect(() => {
    const previousFocus = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
    confirmRef.current?.focus();
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onCloseRef.current();
      if (event.key === "Tab") {
        event.preventDefault();
        confirmRef.current?.focus();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
      previousFocus?.focus();
    };
  }, []);

  return (
    <div
      className="warning-dialog-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <section
        className="warning-dialog"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby={`${id}-title`}
        aria-describedby={`${id}-description`}
      >
        <div className="warning-dialog-icon" aria-hidden="true"><TriangleAlert size={22} /></div>
        <div className="warning-dialog-copy">
          <h2 id={`${id}-title`}>{title}</h2>
          <p id={`${id}-description`}>{description}</p>
        </div>
        <button ref={confirmRef} className="primary-button" type="button" onClick={onClose}>{t("common.understood")}</button>
      </section>
    </div>
  );
}
