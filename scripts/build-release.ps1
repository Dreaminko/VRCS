[CmdletBinding()]
param(
    [ValidatePattern('^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$')]
    [string]$Version,
    [switch]$SkipInstall,
    [switch]$SkipTests
)

$ErrorActionPreference = "Stop"
$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$tauriConfigPath = Join-Path $repoRoot "apps\desktop\src-tauri\tauri.conf.json"
$cargoManifestPath = Join-Path $repoRoot "apps\desktop\src-tauri\Cargo.toml"
$coreManifestPath = Join-Path $repoRoot "core-rust\Cargo.toml"
$tauriConfig = Get-Content -LiteralPath $tauriConfigPath -Raw | ConvertFrom-Json
if (-not $Version) { $Version = $tauriConfig.version }
$cargoVersion = (Select-String -LiteralPath $cargoManifestPath -Pattern '^version = "(.+)"$').Matches[0].Groups[1].Value
$coreVersion = (Select-String -LiteralPath $coreManifestPath -Pattern '^version = "(.+)"$').Matches[0].Groups[1].Value

foreach ($actual in @($tauriConfig.version, $cargoVersion, $coreVersion)) {
    if ($actual -ne $Version) {
        throw "Release version mismatch: requested $Version but a project manifest contains $actual"
    }
}

Push-Location $repoRoot
try {
    if (-not $SkipInstall) {
        & npm ci
        if ($LASTEXITCODE -ne 0) { throw "npm ci failed" }
    }

    if (-not $SkipTests) {
        & (Join-Path $PSScriptRoot "test-core.ps1")
        if ($LASTEXITCODE -ne 0) { throw "Rust Core tests failed" }
        & npm --workspace apps/desktop test
        if ($LASTEXITCODE -ne 0) { throw "Frontend tests failed" }
        & cargo test --manifest-path $cargoManifestPath
        if ($LASTEXITCODE -ne 0) { throw "Desktop Rust tests failed" }
    }

    & npm --workspace apps/desktop run tauri -- build --features cuda --config src-tauri/tauri.release.conf.json --bundles nsis
    if ($LASTEXITCODE -ne 0) { throw "Tauri NSIS build failed" }
}
finally {
    Pop-Location
}

$bundleRoot = Join-Path $repoRoot "apps\desktop\src-tauri\target\release\bundle\nsis"
$installer = Get-ChildItem -LiteralPath $bundleRoot -Filter "*.exe" |
    Sort-Object LastWriteTimeUtc -Descending |
    Select-Object -First 1
if (-not $installer) { throw "No NSIS installer was produced in $bundleRoot" }

$hash = Get-FileHash -LiteralPath $installer.FullName -Algorithm SHA256
$checksumPath = "$($installer.FullName).sha256"
"$($hash.Hash.ToLowerInvariant())  $($installer.Name)" | Set-Content -LiteralPath $checksumPath -Encoding ascii

Write-Host "Release package ready"
Write-Host "  Installer: $($installer.FullName)"
Write-Host "  SHA-256:   $checksumPath"
