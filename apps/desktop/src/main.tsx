import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./app/App";
import { isTauri } from "@tauri-apps/api/core";
import { OcrWindow } from "./ocr/OcrWindow";
import { VisionWindow } from "./vision/VisionWindow";
import { AppErrorBoundary, FatalErrorScreen } from "./shared/ui/AppErrorBoundary";
import {
  installGlobalErrorReporting,
  normalizeFrontendError,
  reportFrontendError,
} from "./app/diagnostics";
import { initializeI18n } from "./i18n";
import { initializeMotionPreference } from "./app/app-motion";
import "./styles.css";

const rootElement = document.getElementById("root")!;

async function render() {
  await initializeI18n();
  const windowLabel = isTauri() ? (await import("@tauri-apps/api/window")).getCurrentWindow().label : "";
  const route = new URLSearchParams(window.location.search).get("window");
  const ocrWindow = route === "ocr" || windowLabel === "ocr";
  const visionWindow = route === "vision" || windowLabel === "vision";
  createRoot(rootElement).render(
    <StrictMode>
      <AppErrorBoundary>
        {visionWindow ? <VisionWindow /> : ocrWindow ? <OcrWindow /> : <App />}
      </AppErrorBoundary>
    </StrictMode>,
  );
}

installGlobalErrorReporting();
initializeMotionPreference();
void render().catch(async (reason) => {
  const error = normalizeFrontendError(reason);
  const reportId = await reportFrontendError({
    kind: "startup",
    operation: "frontend_startup",
    ...error,
  });
  createRoot(rootElement).render(<FatalErrorScreen reportId={reportId} />);
});
