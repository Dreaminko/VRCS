import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { ChevronDown, Download, FolderOpen, Wrench } from "lucide-react";

import {
  exportErrorReport,
  loadDiagnosticStatus,
  openLogDirectory,
  type DiagnosticStatus,
} from "../../app/diagnostics";
import type { DebugRow } from "../settings-types";

function reportFileName(): string {
  const timestamp = new Date().toISOString().replace(/[-:]/g, "").replace("T", "-").slice(0, 15);
  return `VRCS-error-report-${timestamp}.txt`;
}

export function DebugSettingsSection({ rows }: { rows: DebugRow[] }) {
  const { t } = useTranslation();
  const [status, setStatus] = useState<DiagnosticStatus | null>(null);
  const [busy, setBusy] = useState<"open" | "export" | null>(null);
  const [feedback, setFeedback] = useState("");

  useEffect(() => {
    void loadDiagnosticStatus().then(setStatus, () => setStatus(null));
  }, []);

  const openLogs = async () => {
    setBusy("open");
    setFeedback("");
    try {
      await openLogDirectory();
    } catch {
      setFeedback(t("settings.debug.openFailed"));
    } finally {
      setBusy(null);
    }
  };

  const exportReport = async () => {
    setBusy("export");
    setFeedback("");
    try {
      const path = await saveDialog({
        defaultPath: reportFileName(),
        filters: [{ name: "Text", extensions: ["txt"] }],
      });
      if (!path) return;
      await exportErrorReport(path);
      setFeedback(t("settings.debug.exported"));
    } catch {
      setFeedback(t("settings.debug.exportFailed"));
    } finally {
      setBusy(null);
    }
  };

  return (
    <details className="system-settings-group debug-section" aria-labelledby="system-debug-title">
      <summary className="section-heading">
        <div><Wrench size={18} aria-hidden="true" /><h3 id="system-debug-title">Debug</h3></div>
        <ChevronDown className="debug-chevron" size={16} aria-hidden="true" />
      </summary>

      <section className="diagnostic-card">
        <div className="diagnostic-card-copy">
          <strong>{t("settings.debug.errorReports")}</strong>
          <span>{t("settings.debug.errorReportsDescription")}</span>
          {status && <code>{status.logDirectory}</code>}
          {status && <small>{t("settings.debug.sessionId", { id: status.sessionId })}</small>}
          {status?.latestReportId && <small>{t("diagnostics.reportId", { id: status.latestReportId })}</small>}
        </div>
        <div className="diagnostic-actions">
          <button className="secondary-button" type="button" disabled={!status || busy !== null} onClick={() => void openLogs()}>
            <FolderOpen size={16} />
            {t("diagnostics.openLogs")}
          </button>
          <button className="secondary-button" type="button" disabled={!status || busy !== null} onClick={() => void exportReport()}>
            <Download size={16} />
            {busy === "export" ? t("settings.debug.exporting") : t("settings.debug.export")}
          </button>
        </div>
        {feedback && <small className="diagnostic-feedback" role="status">{feedback}</small>}
      </section>

      <div className="debug-list">
        {rows.map((row) => (
          <div className="debug-row" key={row.label}>
            <span>{row.label}</span>
            <strong>{row.value}</strong>
          </div>
        ))}
      </div>
    </details>
  );
}
