import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  build: {
    rollupOptions: {
      input: {
        main: fileURLToPath(new URL("./index.html", import.meta.url)),
        ...(process.env.TAURI_ENV_DEBUG === "true"
          ? { smoke: fileURLToPath(new URL("./tests/vr-player-display.smoke.html", import.meta.url)) }
          : {}),
      },
      output: {
        manualChunks(id) {
          const normalizedId = id.replaceAll("\\", "/");
          if (
            normalizedId.includes("/node_modules/react/")
            || normalizedId.includes("/node_modules/react-dom/")
            || normalizedId.includes("/node_modules/scheduler/")
          ) {
            return "react-runtime";
          }
          return undefined;
        },
      },
    },
  },
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/target/**"],
    },
  },
});

