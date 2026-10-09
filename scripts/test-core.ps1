[CmdletBinding()]
param([switch]$Vulkan)

$ErrorActionPreference = "Stop"
$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$coreManifestPath = Join-Path $repoRoot "core\Cargo.toml"

if ([string]::IsNullOrWhiteSpace($env:CARGO_TARGET_DIR)) {
    Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue
}

$featureArguments = @()
if ($Vulkan) {
    & (Join-Path $PSScriptRoot "prepare-vulkan-runtime.ps1")
    $featureArguments = @("--features", "vulkan")
}
& cargo test --manifest-path $coreManifestPath @featureArguments
if ($LASTEXITCODE -ne 0) { throw "Rust Core tests failed" }
