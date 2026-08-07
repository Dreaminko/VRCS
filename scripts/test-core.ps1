[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$coreManifestPath = Join-Path $repoRoot "core\Cargo.toml"

& cargo test --manifest-path $coreManifestPath
if ($LASTEXITCODE -ne 0) { throw "Rust Core tests failed" }
