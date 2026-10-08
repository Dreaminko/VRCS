[CmdletBinding()]
param()

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

Push-Location $repoRoot
try {
    & (Join-Path $PSScriptRoot "prepare-vulkan-sdk.ps1")
    foreach ($manifest in $manifests) {
        Invoke-CargoCheck -CargoArguments @("fmt", "--manifest-path", $manifest, "--", "--check")
    }
    foreach ($manifest in $manifests) {
        Invoke-CargoCheck -CargoArguments @("clippy", "--manifest-path", $manifest, "--all-targets", "--locked", "--", "-D", "warnings")
    }
    foreach ($manifest in $manifests) {
        Invoke-CargoCheck -CargoArguments @("test", "--manifest-path", $manifest, "--locked")
    }
}
finally {
    Pop-Location
}
