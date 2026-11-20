import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { isTauri } from "@tauri-apps/api/core";
import { AppErrorBoundary, FatalErrorScreen } from "./shared/ui/AppErrorBoundary";
import { ControlTooltip } from "./shared/ui/ControlTooltip";
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
  const parameters = new URLSearchParams(window.location.search);
  const route = parameters.get("window");
  initializeMotionPreference();
  const windowLabel = isTauri() ? (await import("@tauri-apps/api/window")).getCurrentWindow().label : "";
  const vrPreview = import.meta.env.DEV && (route === "vr-preview" || windowLabel === "vr-preview");
  const ocrWindow = route === "ocr" || windowLabel === "ocr";
  const visionWindow = route === "vision" || windowLabel === "vision";
  if (route === "capture-image" || windowLabel === "capture-image") {
    const [, { CaptureImageWindow }] = await Promise.all([
      initializeI18n({ nativePreferences: true }),
      import("./capture-image/CaptureImageWindow"),
    ]);
    createRoot(rootElement).render(<StrictMode><AppErrorBoundary><CaptureImageWindow /></AppErrorBoundary></StrictMode>);
    return;
  }
  if (vrPreview) {
    const [, { VrPreview }] = await Promise.all([
      initializeI18n({ nativePreferences: false }),
      import("./vr-preview/VrPreview"),
    ]);
    createRoot(rootElement).render(<StrictMode><AppErrorBoundary><VrPreview /></AppErrorBoundary></StrictMode>);
    return;
  }
  const [, Component] = await Promise.all([
    initializeI18n({ nativePreferences: true }),
    visionWindow
      ? import("./vision/VisionWindow").then(({ VisionWindow }) => VisionWindow)
      : ocrWindow
        ? import("./ocr/OcrWindow").then(({ OcrWindow }) => OcrWindow)
        : import("./app/App").then(({ default: App }) => App),
  ]);
  createRoot(rootElement).render(
    <StrictMode>
      <AppErrorBoundary>
        <Component />
        <ControlTooltip />
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
