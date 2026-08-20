[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$coreManifestPath = Join-Path $repoRoot "core\Cargo.toml"

if ([string]::IsNullOrWhiteSpace($env:CARGO_TARGET_DIR)) {
    Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue
}

& cargo test --manifest-path $coreManifestPath
if ($LASTEXITCODE -ne 0) { throw "Rust Core tests failed" }
