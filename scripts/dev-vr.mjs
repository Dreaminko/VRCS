import { spawn } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { createServer } from "vite";

const root = fileURLToPath(new URL("../", import.meta.url));

export async function startPreviewServer(port = 1421) {
  const server = await createServer({
    root: fileURLToPath(new URL("../apps/desktop/", import.meta.url)),
    configFile: fileURLToPath(new URL("../apps/desktop/vite.config.ts", import.meta.url)),
    server: { host: "127.0.0.1", port, strictPort: false },
  });
  try {
    await server.listen();
    return server;
  } catch (error) {
    await server.close();
    throw error;
  }
}

async function main() {
  const args = process.argv.slice(2);
  if (args.some((arg) => !["--live", "--capture-smoke"].includes(arg))) throw new Error("Usage: npm run dev:vr -- [--live | --capture-smoke]");
  const live = args.includes("--live");
  const smoke = args.includes("--capture-smoke");
  if (live && smoke) throw new Error("Capture smoke uses isolated mock data");
  const server = await startPreviewServer();
  server.printUrls();
  const configDir = mkdtempSync(join(tmpdir(), "vrcs-vr-preview-"));
  const cleanup = () => rmSync(configDir, { recursive: true, force: true });
  process.once("exit", cleanup);
  try {
    const config = JSON.parse(readFileSync(new URL("../apps/desktop/src-tauri/tauri.conf.json", import.meta.url), "utf8"));
    const override = JSON.stringify({
      identifier: smoke ? "app.vrcs.vr-surface-smoke" : "app.vrcs.vr-preview",
      build: { beforeDevCommand: "", devUrl: server.resolvedUrls.local[0] },
      app: { windows: config.app.windows.map((window) => ({ ...window, create: live })) },
    });
    // Pass a file path through Windows PowerShell, which does not preserve JSON argument quotes.
    const configPath = join(configDir, "tauri.preview.json");
    writeFileSync(configPath, override);
    const child = spawn("powershell.exe", ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File",
      fileURLToPath(new URL("run-tauri.ps1", import.meta.url)), "dev", "--features", "vr-preview", "--config", configPath,
      ...(smoke ? ["--no-watch"] : []),
      "--", "--bin", smoke ? "vrcs-vr-surface-smoke" : "vrcs-vr-preview"], {
      cwd: `${root}/apps/desktop`, stdio: "inherit", windowsHide: true,
      env: { ...process.env, VRCS_VR_PREVIEW: live ? "live" : "mock", ...(smoke ? { VRCS_VR_SURFACE_SMOKE: "1" } : {}) },
    });
    let stopping = false;
    function stop() {
      if (stopping || !child.pid) return;
      stopping = true;
      spawn("taskkill.exe", ["/PID", String(child.pid), "/T", "/F"], { stdio: "ignore", windowsHide: true });
    }
    process.once("SIGINT", stop);
    process.once("SIGTERM", stop);
    process.once("exit", stop);
    try {
      process.exitCode = await new Promise((resolve, reject) => {
        child.once("error", reject);
        child.once("exit", (code) => resolve(code ?? (stopping ? 0 : 1)));
      });
    } finally {
      process.removeListener("SIGINT", stop);
      process.removeListener("SIGTERM", stop);
      process.removeListener("exit", stop);
    }
  } finally {
    await server.close();
    cleanup();
    process.removeListener("exit", cleanup);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => { console.error(error.message); process.exitCode = 1; });
}
