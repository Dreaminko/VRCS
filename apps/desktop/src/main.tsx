import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./app/App";
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
  createRoot(rootElement).render(
    <StrictMode>
      <AppErrorBoundary>
        <App />
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
