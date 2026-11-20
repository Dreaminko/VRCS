[CmdletBinding()]
param(
    [switch]$Vulkan,
    [switch]$CiVulkanOnly,
    [ValidateSet("all", "format", "clippy", "test")]
    [string]$Check = "all"
)

$ErrorActionPreference = "Stop"
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

Push-Location $repoRoot
try {
    if ($Check -in @("all", "format")) {
        Invoke-CargoCheck -CargoArguments @("fmt", "--all", "--", "--check")
    }
    if ($Check -eq "format") { return }
    if ($env:OS -ne "Windows_NT") {
        throw "Run Rust checks on Windows to include the desktop capture and overlay code"
    }
    if ($Vulkan) {
        & (Join-Path $PSScriptRoot "prepare-vulkan-runtime.ps1")
    }
    if ($Check -in @("all", "clippy")) {
        if ($Vulkan) {
            Invoke-CargoCheck -CargoArguments @("clippy", "-p", "vrcs-core", "-p", "vrcs-desktop", "--all-targets", "--locked", "--features", "vulkan", "--", "-D", "warnings")
            if (-not $CiVulkanOnly) {
                Invoke-CargoCheck -CargoArguments @("clippy", "-p", "player-audio-scan", "-p", "player-audio-helper", "--all-targets", "--locked", "--", "-D", "warnings")
            }
        }
        else {
            Invoke-CargoCheck -CargoArguments @("clippy", "--workspace", "--all-targets", "--locked", "--", "-D", "warnings")
            Invoke-CargoCheck -CargoArguments @("clippy", "-p", "vrcs-desktop", "--no-deps", "--all-targets", "--locked", "--features", "vr-preview", "--", "-D", "warnings")
        }
    }
    if ($Check -in @("all", "test")) {
        if ($CiVulkanOnly) {
            # Vulkan changes only the managed Qwen runtime; CPU CI runs the full suite.
            Invoke-CargoCheck -CargoArguments @("test", "-p", "vrcs-core", "--locked", "--features", "vulkan", "asr::qwen_runtime::tests::")
        }
        elseif ($Vulkan) {
            Invoke-CargoCheck -CargoArguments @("test", "-p", "vrcs-core", "-p", "vrcs-desktop", "--locked", "--features", "vulkan")
            Invoke-CargoCheck -CargoArguments @("test", "-p", "player-audio-scan", "-p", "player-audio-helper", "--locked")
        }
        else {
            Invoke-CargoCheck -CargoArguments @("test", "--workspace", "--locked")
        }
    }
}
finally {
    Pop-Location
}
