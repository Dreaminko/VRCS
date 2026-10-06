import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./app/App";
import { isTauri } from "@tauri-apps/api/core";
import { OcrWindow } from "./ocr/OcrWindow";
import { AppErrorBoundary, FatalErrorScreen } from "./shared/ui/AppErrorBoundary";
import {
  installGlobalErrorReporting,
  normalizeFrontendError,
  reportFrontendError,
} from "./diagnostics";
import { initializeI18n } from "./i18n";
import "./styles.css";

const rootElement = document.getElementById("root")!;

async function render() {
  await initializeI18n();
  const ocrWindow = new URLSearchParams(window.location.search).get("window") === "ocr"
    || (isTauri() && (await import("@tauri-apps/api/window")).getCurrentWindow().label === "ocr");
  createRoot(rootElement).render(
    <StrictMode>
      <AppErrorBoundary>
        {ocrWindow ? <OcrWindow /> : <App />}
      </AppErrorBoundary>
    </StrictMode>,
  );
}

installGlobalErrorReporting();
void render().catch(async (reason) => {
  const error = normalizeFrontendError(reason);
  const reportId = await reportFrontendError({
    kind: "startup",
    operation: "frontend_startup",
    ...error,
  });
  createRoot(rootElement).render(<FatalErrorScreen reportId={reportId} />);
});
