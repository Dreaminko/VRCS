import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { initializeI18n } from "./i18n";
import "./styles.css";

async function render() {
  await initializeI18n();
  createRoot(document.getElementById("root")!).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}

void render();
