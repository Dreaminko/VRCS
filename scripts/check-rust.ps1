[CmdletBinding()]
param([switch]$Vulkan)

$ErrorActionPreference = "Stop"
if ($env:OS -ne "Windows_NT") {
    throw "Run Rust checks on Windows to include the desktop capture and overlay code"
}

function Invoke-CargoCheck {
    param([string[]]$CargoArguments)

    & cargo @CargoArguments
    if ($LASTEXITCODE -ne 0) {
        throw "cargo $($CargoArguments -join ' ') failed with exit code $LASTEXITCODE"
    }
}

$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$manifests = @("core/Cargo.toml", "apps/desktop/src-tauri/Cargo.toml")
$featureArguments = if ($Vulkan) { @("--features", "vulkan") } else { @() }

Push-Location $repoRoot
try {
    if ($Vulkan) {
        & (Join-Path $PSScriptRoot "prepare-vulkan-sdk.ps1")
    }
    foreach ($manifest in $manifests) {
        Invoke-CargoCheck -CargoArguments @("fmt", "--manifest-path", $manifest, "--", "--check")
    }
    foreach ($manifest in $manifests) {
        Invoke-CargoCheck -CargoArguments (@("clippy", "--manifest-path", $manifest, "--all-targets", "--locked") + $featureArguments + @("--", "-D", "warnings"))
    }
    foreach ($manifest in $manifests) {
        Invoke-CargoCheck -CargoArguments (@("test", "--manifest-path", $manifest, "--locked") + $featureArguments)
    }
}
finally {
    Pop-Location
}
