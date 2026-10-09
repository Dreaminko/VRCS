$ErrorActionPreference = "Stop"

# Stage the Vulkan loader when Qwen GPU support is requested.
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
if ($allFeatures -or "vulkan" -in $features) {
    & (Join-Path $PSScriptRoot "prepare-vulkan-runtime.ps1")
}
$tauriCli = Join-Path $PSScriptRoot "..\node_modules\@tauri-apps\cli\tauri.js"
& node $tauriCli @args
exit $LASTEXITCODE
