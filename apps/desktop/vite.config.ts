import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { createHash } from "node:crypto";
import { fileURLToPath } from "node:url";

const surfaceCsp = {
  name: "vr-surface-dev-csp",
  enforce: "post" as const,
  transformIndexHtml: {
    order: "post" as const,
    handler(html: string, context: import("vite").IndexHtmlTransformContext) {
      if (!context.server || context.path !== "/vr-surface.html") return html;
      const origin = context.server.resolvedUrls?.local[0];
      if (!origin) throw new Error("Surface dev origin is unavailable");
      const http = new URL(origin).origin;
      const ws = http.replace(/^http/, "ws");
      const hashes = [...html.matchAll(/<script\b([^>]*)>([\s\S]*?)<\/script>/gi)]
        .filter(([, attributes, source]) => !/\bsrc\s*=/.test(attributes) && source.trim())
        .map(([, , source]) => `'sha256-${createHash("sha256").update(source).digest("base64")}'`);
      const policy = `default-src 'none'; script-src 'self' ${hashes.join(" ")}; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self'; connect-src ${http} ${ws}; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'`;
      return html.replace(/<head>/i, `<head>\n    <meta http-equiv="Content-Security-Policy" content="${policy}" />`);
    },
  },
};

export default defineConfig({
  plugins: [react(), surfaceCsp],
  clearScreen: false,
  build: {
    rollupOptions: {
      input: {
        main: fileURLToPath(new URL("./index.html", import.meta.url)),
        surface: fileURLToPath(new URL("./vr-surface.html", import.meta.url)),
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

