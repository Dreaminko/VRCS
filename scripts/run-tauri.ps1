$ErrorActionPreference = "Stop"

# Keep GGML's nested shader build below the Windows path limit.
if (-not $env:CARGO_TARGET_DIR) {
    $env:CARGO_TARGET_DIR = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot "..\core\target"))
}
# Prepare native GPU tools only when the requested Cargo features need them.
$features = @()
$allFeatures = $args -contains "--all-features"
for ($index = 0; $index -lt $args.Count; $index++) {
    $argument = $args[$index]
    if ($argument -in @("--features", "-f", "-F")) {
        while ($index + 1 -lt $args.Count -and -not $args[$index + 1].StartsWith("-")) {
            $index++
            $features += $args[$index] -split '[,\s]+'
        }
    }
    elseif ($argument -match '^--features=(.+)$') {
        $features += $Matches[1] -split '[,\s]+'
    }
    elseif ($argument -match '^-[fF]=?(.+)$') {
        $features += $Matches[1] -split '[,\s]+'
    }
}
if ($allFeatures -or "vulkan" -in $features -or "cuda" -in $features) {
    & (Join-Path $PSScriptRoot "prepare-vulkan-sdk.ps1")
}
$tauriCli = Join-Path $PSScriptRoot "..\node_modules\@tauri-apps\cli\tauri.js"
& node $tauriCli @args
exit $LASTEXITCODE
