$ErrorActionPreference = "Stop"

# Keep GGML's nested shader build below the Windows path limit.
if (-not $env:CARGO_TARGET_DIR) {
    $env:CARGO_TARGET_DIR = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot "..\core\target"))
}
& (Join-Path $PSScriptRoot "prepare-vulkan-sdk.ps1")
$tauriCli = Join-Path $PSScriptRoot "..\node_modules\@tauri-apps\cli\tauri.js"
& node $tauriCli @args
exit $LASTEXITCODE
