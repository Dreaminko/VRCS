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
$tauriReleaseConfigTemplatePath = Join-Path $repoRoot "apps\desktop\src-tauri\tauri.release.conf.json"
$generatedTauriConfigPath = Join-Path $repoRoot "apps\desktop\src-tauri\tauri.release.generated.conf.json"
$generatedTauriConfigArgument = "src-tauri/tauri.release.generated.conf.json"
$cargoManifestPath = Join-Path $repoRoot "apps\desktop\src-tauri\Cargo.toml"
$desktopPackagePath = Join-Path $repoRoot "apps\desktop\package.json"
$coreManifestPath = Join-Path $repoRoot "core\Cargo.toml"
$defaultTargetRoot = Join-Path $repoRoot ".build"
$artifactRoot = Join-Path $repoRoot "release-artifacts"
$tauriConfig = Get-Content -LiteralPath $tauriConfigPath -Raw | ConvertFrom-Json
if ([string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY)) { throw "TAURI_SIGNING_PRIVATE_KEY is required for release builds" }
if ([string]::IsNullOrWhiteSpace($env:TAURI_UPDATER_PUBLIC_KEY)) { throw "TAURI_UPDATER_PUBLIC_KEY is required for release builds" }
if (-not $Version) { $Version = $tauriConfig.version }
$cargoVersion = (Select-String -LiteralPath $cargoManifestPath -Pattern '^version = "(.+)"$').Matches[0].Groups[1].Value
$desktopPackageVersion = (Get-Content -LiteralPath $desktopPackagePath -Raw | ConvertFrom-Json).version
$coreVersion = (Select-String -LiteralPath $coreManifestPath -Pattern '^version = "(.+)"$').Matches[0].Groups[1].Value

foreach ($actual in @($tauriConfig.version, $cargoVersion, $desktopPackageVersion, $coreVersion)) {
    if ($actual -ne $Version) {
        throw "Release version mismatch: requested $Version but a project manifest contains $actual"
    }
}

function Assert-NonEmptyFile {
    param([Parameter(Mandatory)][string]$Path)

    $file = Get-Item -LiteralPath $Path -ErrorAction SilentlyContinue
    if (-not $file -or $file.Length -eq 0) {
        throw "Expected a non-empty file: $Path"
    }
}

function Write-ReleaseTauriConfig {
    param(
        [Parameter(Mandatory)][string]$TemplatePath,
        [Parameter(Mandatory)][string]$DestinationPath,
        [Parameter(Mandatory)][string]$PublicKey
    )

    $releaseConfig = Get-Content -LiteralPath $TemplatePath -Raw | ConvertFrom-Json
    $releaseConfig | Add-Member -Force -NotePropertyName "plugins" -NotePropertyValue ([PSCustomObject]@{
        updater = [PSCustomObject]@{
            pubkey = $PublicKey
            endpoints = @("https://github.com/Dreaminko/VRCS/releases/latest/download/latest.json")
        }
    })
    $releaseConfigJson = $releaseConfig | ConvertTo-Json -Depth 10
    $utf8WithoutBom = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($DestinationPath, $releaseConfigJson, $utf8WithoutBom)
}

function Invoke-ReleaseBuild {
    param(
        [Parameter(Mandatory)]
        [string]$Label,
        [string[]]$Features = @()
    )

    $buildTargetRoot = $defaultTargetRoot
    $bundleRoot = Join-Path $buildTargetRoot "release\bundle\nsis"

    $arguments = @(
        "--workspace", "apps/desktop",
        "run", "tauri", "--",
        "build"
    )
    if ($Features.Count -gt 0) {
        $arguments += @("--features", ($Features -join ","))
    }
    $arguments += @(
        "--config", $generatedTauriConfigArgument,
        "--bundles", "nsis"
    )

    $previousCargoTargetDir = [Environment]::GetEnvironmentVariable("CARGO_TARGET_DIR", "Process")
    try {
        $env:CARGO_TARGET_DIR = $buildTargetRoot
        $resolvedBundleRoot = [System.IO.Path]::GetFullPath($bundleRoot)
        $resolvedTargetRoot = [System.IO.Path]::GetFullPath($buildTargetRoot).TrimEnd('\') + '\'
        if (-not $resolvedBundleRoot.StartsWith($resolvedTargetRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
            throw "Bundle cleanup path is outside the build target"
        }
        if (Test-Path -LiteralPath $resolvedBundleRoot) {
            Remove-Item -LiteralPath $resolvedBundleRoot -Recurse -Force
        }
        New-Item -ItemType Directory -Path $bundleRoot -Force | Out-Null

        Write-Host "Building $Label release"
        & npm @arguments | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "$Label Tauri NSIS build failed" }
    }
    finally {
        [Environment]::SetEnvironmentVariable("CARGO_TARGET_DIR", $previousCargoTargetDir, "Process")
    }

    $desktopExecutable = Join-Path $buildTargetRoot "release\vrcs-desktop.exe"
    if (-not (Test-Path -LiteralPath $desktopExecutable -PathType Leaf)) {
        throw "Built desktop executable not found: $desktopExecutable"
    }
    $selfTest = Start-Process -FilePath $desktopExecutable -ArgumentList "--release-self-test" -Wait -PassThru -WindowStyle Hidden
    if ($selfTest.ExitCode -ne 0) { throw "$Label release self-test failed with exit code $($selfTest.ExitCode)" }

    $installers = @(Get-ChildItem -LiteralPath $bundleRoot -Filter "*.exe" -File)
    if ($installers.Count -ne 1) {
        throw "Expected exactly one NSIS installer in $bundleRoot, found $($installers.Count)"
    }
    $installer = $installers[0]
    Assert-NonEmptyFile $installer.FullName

    $installerSignature = "$($installer.FullName).sig"
    Assert-NonEmptyFile $installerSignature

    New-Item -ItemType Directory -Path $artifactRoot -Force | Out-Null
    $artifactName = "VRCS-$Version-windows-x64.exe"
    $artifactPath = Join-Path $artifactRoot $artifactName
    $signaturePath = "$artifactPath.sig"
    Copy-Item -LiteralPath $installer.FullName -Destination $artifactPath -Force
    Copy-Item -LiteralPath $installerSignature -Destination $signaturePath -Force
    Assert-NonEmptyFile $artifactPath
    Assert-NonEmptyFile $signaturePath

    $hash = Get-FileHash -LiteralPath $artifactPath -Algorithm SHA256
    $checksumPath = "$artifactPath.sha256"
    "$($hash.Hash.ToLowerInvariant())  $artifactName" | Set-Content -LiteralPath $checksumPath -Encoding ascii
    Assert-NonEmptyFile $checksumPath

    return [PSCustomObject]@{
        ArtifactName = $artifactName
        ArtifactPath = $artifactPath
        SignaturePath = $signaturePath
        ChecksumPath = $checksumPath
    }
}

$previousReleaseCargoTargetDir = [Environment]::GetEnvironmentVariable("CARGO_TARGET_DIR", "Process")
Push-Location $repoRoot
try {
    $env:CARGO_TARGET_DIR = $defaultTargetRoot
    & (Join-Path $PSScriptRoot "prepare-vulkan-runtime.ps1")
    Write-ReleaseTauriConfig `
        -TemplatePath $tauriReleaseConfigTemplatePath `
        -DestinationPath $generatedTauriConfigPath `
        -PublicKey $env:TAURI_UPDATER_PUBLIC_KEY

    if (-not $SkipInstall) {
        & npm ci
        if ($LASTEXITCODE -ne 0) { throw "npm ci failed" }
    }

    if (-not $SkipTests) {
        & (Join-Path $PSScriptRoot "test-core.ps1") -Vulkan
        if ($LASTEXITCODE -ne 0) { throw "Rust Core tests failed" }
        & npm --workspace apps/desktop test
        if ($LASTEXITCODE -ne 0) { throw "Frontend tests failed" }
        & cargo test --manifest-path $cargoManifestPath --features vulkan
        if ($LASTEXITCODE -ne 0) { throw "Desktop Rust tests failed" }
    }

    $releaseBuilds = @(Invoke-ReleaseBuild -Label "standard" -Features @("vulkan"))
    $latestPath = & (Join-Path $PSScriptRoot "write-release-metadata.ps1") -Version $Version -ArtifactRoot $artifactRoot
    Assert-NonEmptyFile $latestPath

    $releaseArtifacts = @($releaseBuilds | ForEach-Object {
        $_.ArtifactPath
        $_.SignaturePath
        $_.ChecksumPath
    }) + $latestPath
}
finally {
    [Environment]::SetEnvironmentVariable("CARGO_TARGET_DIR", $previousReleaseCargoTargetDir, "Process")
    if (Test-Path -LiteralPath $generatedTauriConfigPath) {
        Remove-Item -LiteralPath $generatedTauriConfigPath -Force
    }
    Pop-Location
}

Write-Host "Release packages ready"
foreach ($artifact in $releaseArtifacts) {
    Write-Host "  $artifact"
}
