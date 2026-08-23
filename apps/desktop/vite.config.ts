import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  build: {
    rollupOptions: {
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

