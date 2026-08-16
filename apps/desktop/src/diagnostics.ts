import { invoke, isTauri } from "@tauri-apps/api/core";

export interface DiagnosticStatus {
  logDirectory: string;
  sessionId: string;
  latestReportId: string | null;
}

export interface FrontendErrorDetails {
  kind: "render" | "window_error" | "unhandled_rejection" | "startup";
  operation: string;
  message: string;
  stack?: string;
  componentStack?: string;
}

const recentReports = new Map<string, number>();
let installed = false;

export function normalizeFrontendError(reason: unknown): { message: string; stack?: string } {
  if (reason instanceof Error) {
    return { message: reason.message || reason.name, stack: reason.stack };
  }
  if (typeof reason === "string") return { message: reason };
  return { message: "Unknown frontend error" };
}

export async function reportFrontendError(
  details: FrontendErrorDetails,
): Promise<string | null> {
  const key = `${details.kind}:${details.operation}:${details.message}`;
  const now = Date.now();
  const previous = recentReports.get(key);
  if (previous !== undefined && now - previous < 2_000) return null;
  recentReports.set(key, now);

  if (!isTauri()) {
    console.error("VRCS frontend error", details);
    return null;
  }

  try {
    return await invoke<string>("report_frontend_error", { report: details });
  } catch (error) {
    console.error("Failed to write the VRCS frontend error log", error);
    return null;
  }
}

export function installGlobalErrorReporting(): void {
  if (installed) return;
  installed = true;

  window.addEventListener("error", (event) => {
    const error = normalizeFrontendError(event.error ?? event.message);
    void reportFrontendError({
      kind: "window_error",
      operation: "window_error",
      ...error,
    });
  });

  window.addEventListener("unhandledrejection", (event) => {
    const error = normalizeFrontendError(event.reason);
    void reportFrontendError({
      kind: "unhandled_rejection",
      operation: "unhandled_rejection",
      ...error,
    });
  });
}

export async function loadDiagnosticStatus(): Promise<DiagnosticStatus | null> {
  if (!isTauri()) return null;
  return invoke<DiagnosticStatus>("diagnostic_status");
}

export async function openLogDirectory(): Promise<void> {
  if (!isTauri()) return;
  await invoke("open_log_directory");
}

export async function exportErrorReport(path: string): Promise<void> {
  if (!isTauri()) return;
  await invoke("export_error_report", { path });
}
