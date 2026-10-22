import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { isTauri } from "@tauri-apps/api/core";
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
  const parameters = new URLSearchParams(window.location.search);
  const route = parameters.get("window");
  if (route === "vr-surface") {
    const surface = parameters.get("surface");
    if (surface !== "menu" && surface !== "dialogue" && surface !== "reader") throw new Error("Unknown VR surface");
    document.documentElement.dataset.vrSurface = surface;
    const { SurfaceHost } = await import("./vr-surface/SurfaceHost");
    createRoot(rootElement).render(<StrictMode><SurfaceHost surface={surface} /></StrictMode>);
    return;
  }
  initializeMotionPreference();
  const windowLabel = isTauri() ? (await import("@tauri-apps/api/window")).getCurrentWindow().label : "";
  const vrPreview = import.meta.env.DEV && (route === "vr-preview" || windowLabel === "vr-preview");
  await initializeI18n({ nativePreferences: !vrPreview });
  const ocrWindow = route === "ocr" || windowLabel === "ocr";
  const visionWindow = route === "vision" || windowLabel === "vision";
  if (vrPreview) {
    const { VrPreview } = await import("./vr-preview/VrPreview");
    createRoot(rootElement).render(<StrictMode><AppErrorBoundary><VrPreview /></AppErrorBoundary></StrictMode>);
    return;
  }
  const Component = visionWindow ? (await import("./vision/VisionWindow")).VisionWindow
    : ocrWindow ? (await import("./ocr/OcrWindow")).OcrWindow
      : (await import("./app/App")).default;
  createRoot(rootElement).render(
    <StrictMode>
      <AppErrorBoundary>
        <Component />
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
