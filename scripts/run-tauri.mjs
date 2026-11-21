import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";

const args = process.argv.slice(2);
let command;
let commandArgs;

switch (process.platform) {
  case "win32":
    command = "powershell";
    commandArgs = [
      "-NoProfile",
      "-ExecutionPolicy", "Bypass",
      "-File", fileURLToPath(new URL("./run-tauri.ps1", import.meta.url)),
      ...args,
    ];
    break;
  case "linux":
    command = process.execPath;
    commandArgs = [
      fileURLToPath(new URL("../node_modules/@tauri-apps/cli/tauri.js", import.meta.url)),
      ...args,
    ];
    break;
  default:
    console.error(`Tauri development is not supported on platform: ${process.platform}`);
    process.exit(1);
}

const child = spawn(command, commandArgs, { stdio: "inherit" });
const forwardInterrupt = () => child.kill("SIGINT");
const forwardTermination = () => child.kill("SIGTERM");
process.on("SIGINT", forwardInterrupt);
process.on("SIGTERM", forwardTermination);
child.on("error", (error) => {
  console.error(`Failed to launch Tauri: ${error.message}`);
  process.exitCode = 1;
});
child.on("close", (code, signal) => {
  process.off("SIGINT", forwardInterrupt);
  process.off("SIGTERM", forwardTermination);
  if (signal) {
    process.kill(process.pid, signal);
  } else {
    process.exitCode = code ?? 1;
  }
});
