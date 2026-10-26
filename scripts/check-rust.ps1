[CmdletBinding()]
param(
    [switch]$Vulkan,
    [switch]$CiVulkanOnly,
    [ValidateSet("all", "format", "clippy", "test")]
    [string]$Check = "all"
)

$ErrorActionPreference = "Stop"
if ($env:OS -ne "Windows_NT") {
    throw "Run Rust checks on Windows to include the desktop capture and overlay code"
}
if ($CiVulkanOnly -and -not $Vulkan) {
    throw "CiVulkanOnly requires -Vulkan"
}

function Invoke-CargoCheck {
    param([string[]]$CargoArguments)

    & cargo @CargoArguments
    if ($LASTEXITCODE -ne 0) {
        throw "cargo $($CargoArguments -join ' ') failed with exit code $LASTEXITCODE"
    }
}

$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$manifests = @("core/Cargo.toml", "apps/desktop/src-tauri/Cargo.toml", "crates/player-audio-scan/Cargo.toml")
$checkManifests = if ($CiVulkanOnly) { $manifests[0..1] } else { $manifests }
$featureArguments = if ($Vulkan) { @("--features", "vulkan") } else { @() }

Push-Location $repoRoot
try {
    if ($Check -in @("all", "format")) {
        foreach ($manifest in $manifests) {
            Invoke-CargoCheck -CargoArguments @("fmt", "--manifest-path", $manifest, "--", "--check")
        }
    }
    if ($Check -eq "format") { return }
    if ($Vulkan) {
        & (Join-Path $PSScriptRoot "prepare-vulkan-runtime.ps1")
    }
    if ($Check -in @("all", "clippy")) {
        foreach ($manifest in $checkManifests) {
            $features = if ($manifest.StartsWith("crates/")) { @() } else { $featureArguments }
            Invoke-CargoCheck -CargoArguments (@("clippy", "--manifest-path", $manifest, "--all-targets", "--locked") + $features + @("--", "-D", "warnings"))
        }
    }
    if ($Check -in @("all", "test")) {
        foreach ($manifest in $checkManifests) {
            $features = if ($manifest.StartsWith("crates/")) { @() } else { $featureArguments }
            $testArguments = if ($CiVulkanOnly) { @("--all-targets", "--no-run") } else { @() }
            Invoke-CargoCheck -CargoArguments (@("test", "--manifest-path", $manifest) + $testArguments + @("--locked") + $features)
        }
        if ($CiVulkanOnly) {
            # Vulkan changes only the managed Qwen runtime; CPU CI runs the full suite.
            Invoke-CargoCheck -CargoArguments @("test", "--manifest-path", "core/Cargo.toml", "--locked", "--features", "vulkan", "asr::qwen_runtime::tests::")
        }
    }
}
finally {
    Pop-Location
}
