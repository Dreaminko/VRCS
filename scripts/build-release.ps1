[CmdletBinding()]
param(
    [ValidatePattern('^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$')]
    [string]$Version,
    [switch]$SkipInstall,
    [switch]$SkipTests,
    [switch]$IncludeCuda
)

$ErrorActionPreference = "Stop"
$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$tauriConfigPath = Join-Path $repoRoot "apps\desktop\src-tauri\tauri.conf.json"
$cargoManifestPath = Join-Path $repoRoot "apps\desktop\src-tauri\Cargo.toml"
$coreManifestPath = Join-Path $repoRoot "core-rust\Cargo.toml"
$bundleRoot = Join-Path $repoRoot "apps\desktop\src-tauri\target\release\bundle\nsis"
$artifactRoot = Join-Path $repoRoot "release-artifacts"
$tauriConfig = Get-Content -LiteralPath $tauriConfigPath -Raw | ConvertFrom-Json
if (-not $Version) { $Version = $tauriConfig.version }
$cargoVersion = (Select-String -LiteralPath $cargoManifestPath -Pattern '^version = "(.+)"$').Matches[0].Groups[1].Value
$coreVersion = (Select-String -LiteralPath $coreManifestPath -Pattern '^version = "(.+)"$').Matches[0].Groups[1].Value

foreach ($actual in @($tauriConfig.version, $cargoVersion, $coreVersion)) {
    if ($actual -ne $Version) {
        throw "Release version mismatch: requested $Version but a project manifest contains $actual"
    }
}

function Invoke-ReleaseBuild {
    param(
        [Parameter(Mandatory)]
        [string]$Label,
        [string[]]$Features = @(),
        [string]$FileSuffix = ""
    )

    $arguments = @(
        "--workspace", "apps/desktop",
        "run", "tauri", "--",
        "build"
    )
    if ($Features.Count -gt 0) {
        $arguments += @("--features", ($Features -join ","))
    }
    $arguments += @(
        "--config", "src-tauri/tauri.release.conf.json",
        "--bundles", "nsis"
    )

    Write-Host "Building $Label release"
    & npm @arguments | Out-Host
    if ($LASTEXITCODE -ne 0) { throw "$Label Tauri NSIS build failed" }

    $desktopExecutable = Join-Path $repoRoot "apps\desktop\src-tauri\target\release\vrcs-desktop.exe"
    if (-not (Test-Path -LiteralPath $desktopExecutable -PathType Leaf)) {
        throw "Built desktop executable not found: $desktopExecutable"
    }
    $selfTest = Start-Process -FilePath $desktopExecutable -ArgumentList "--release-self-test" -Wait -PassThru -WindowStyle Hidden
    if ($selfTest.ExitCode -ne 0) { throw "$Label release self-test failed with exit code $($selfTest.ExitCode)" }

    $installer = Get-ChildItem -LiteralPath $bundleRoot -Filter "*.exe" |
        Sort-Object LastWriteTimeUtc -Descending |
        Select-Object -First 1
    if (-not $installer) { throw "No NSIS installer was produced in $bundleRoot" }

    New-Item -ItemType Directory -Path $artifactRoot -Force | Out-Null
    $artifactName = "VRCS-$Version-windows-x64$FileSuffix.exe"
    $artifactPath = Join-Path $artifactRoot $artifactName
    Copy-Item -LiteralPath $installer.FullName -Destination $artifactPath -Force

    $hash = Get-FileHash -LiteralPath $artifactPath -Algorithm SHA256
    $checksumPath = "$artifactPath.sha256"
    "$($hash.Hash.ToLowerInvariant())  $artifactName" | Set-Content -LiteralPath $checksumPath -Encoding ascii

    return @($artifactPath, $checksumPath)
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

    $releaseArtifacts = @(Invoke-ReleaseBuild -Label "standard")
    if ($IncludeCuda) {
        $releaseArtifacts += Invoke-ReleaseBuild -Label "CUDA" -Features @("cuda") -FileSuffix "-CUDA"
    }
}
finally {
    Pop-Location
}

Write-Host "Release packages ready"
foreach ($artifact in $releaseArtifacts) {
    Write-Host "  $artifact"
}
