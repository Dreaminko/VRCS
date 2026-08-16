import { Component } from "react";
import type { ErrorInfo, ReactNode } from "react";
import { FolderOpen, RotateCcw, TriangleAlert } from "lucide-react";

import { openLogDirectory, reportFrontendError } from "../diagnostics";
import i18n from "../i18n";

interface State {
  failed: boolean;
  reportId: string | null;
}

export function FatalErrorScreen({ reportId }: { reportId: string | null }) {
  return (
    <main className="fatal-error-screen" role="alert">
      <section className="fatal-error-card">
        <div className="fatal-error-icon" aria-hidden="true"><TriangleAlert size={24} /></div>
        <div className="fatal-error-copy">
          <h1>{i18n.t("diagnostics.fatalTitle", { defaultValue: "VRCS encountered an unexpected error" })}</h1>
          <p>{i18n.t("diagnostics.fatalDescription", { defaultValue: "The error was saved locally. Reload the interface or open the log folder for details." })}</p>
          {reportId && (
            <small>{i18n.t("diagnostics.reportId", { id: reportId, defaultValue: `Report ID: ${reportId}` })}</small>
          )}
        </div>
        <div className="fatal-error-actions">
          <button className="secondary-button" type="button" onClick={() => void openLogDirectory().catch(() => undefined)}>
            <FolderOpen size={16} />
            {i18n.t("diagnostics.openLogs", { defaultValue: "Open logs" })}
          </button>
          <button className="primary-button" type="button" onClick={() => window.location.reload()}>
            <RotateCcw size={16} />
            {i18n.t("diagnostics.reload", { defaultValue: "Reload" })}
          </button>
        </div>
      </section>
    </main>
  );
}

export class AppErrorBoundary extends Component<{ children: ReactNode }, State> {
  state: State = { failed: false, reportId: null };

  static getDerivedStateFromError(): State {
    return { failed: true, reportId: null };
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    void reportFrontendError({
      kind: "render",
      operation: "react_render",
      message: error.message || error.name,
      stack: error.stack,
      componentStack: info.componentStack ?? undefined,
    }).then((reportId) => {
      if (reportId) this.setState({ reportId });
    });
  }

  render() {
    if (!this.state.failed) return this.props.children;

    return <FatalErrorScreen reportId={this.state.reportId} />;
  }
}
