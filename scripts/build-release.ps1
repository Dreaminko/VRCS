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
$tauriReleaseConfigTemplatePath = Join-Path $repoRoot "apps\desktop\src-tauri\tauri.release.conf.json"
$generatedTauriConfigPath = Join-Path $repoRoot "apps\desktop\src-tauri\tauri.release.generated.conf.json"
$generatedTauriConfigArgument = "src-tauri/tauri.release.generated.conf.json"
$cargoManifestPath = Join-Path $repoRoot "apps\desktop\src-tauri\Cargo.toml"
$desktopPackagePath = Join-Path $repoRoot "apps\desktop\package.json"
$coreManifestPath = Join-Path $repoRoot "core\Cargo.toml"
$defaultTargetRoot = Join-Path $repoRoot "apps\desktop\src-tauri\target"
$artifactRoot = Join-Path $repoRoot "release-artifacts"
$cudaArchitectures = "75-real;80-real;86-real;89-real;89-virtual;120a-real"
$requiredCudaArchitectures = @("sm_75", "sm_80", "sm_86", "sm_89", "sm_120a")
$cudaToolkitVersion = $null
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

function Resolve-CudaTool {
    param([Parameter(Mandatory)][string]$Name)

    $command = Get-Command $Name -ErrorAction SilentlyContinue
    if ($command) {
        return $command.Source
    }
    if ($env:CUDA_PATH) {
        foreach ($directory in @("bin\x64", "bin")) {
            $candidate = Join-Path $env:CUDA_PATH "$directory\$Name"
            if (Test-Path -LiteralPath $candidate -PathType Leaf) {
                return $candidate
            }
        }
    }
    throw "$Name is unavailable; install the CUDA 13.x Toolkit and configure CUDA_PATH"
}

function Assert-Cuda13Toolchain {
    $nvcc = Resolve-CudaTool "nvcc.exe"
    $versionOutput = (& $nvcc --version 2>&1 | Out-String)
    if ($LASTEXITCODE -ne 0) {
        throw "nvcc failed with exit code $LASTEXITCODE"
    }
    $versionMatch = [regex]::Match($versionOutput, 'release\s+(13\.\d+)')
    if (-not $versionMatch.Success) {
        throw "CUDA 13.x is required for CUDA release builds; nvcc reported: $($versionOutput.Trim())"
    }
    $script:cudaToolkitVersion = $versionMatch.Groups[1].Value
    Write-Host "Using CUDA $cudaToolkitVersion with CUDAARCHS=$cudaArchitectures"
}

function Get-CudaReleaseTargetRoot {
    if (-not $cudaToolkitVersion) {
        throw "CUDA toolchain validation must run before selecting the CUDA release target"
    }
    $identity = "$cudaToolkitVersion|$cudaArchitectures"
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($identity)
    $sha256 = [System.Security.Cryptography.SHA256]::Create()
    try {
        $hash = [Convert]::ToHexString($sha256.ComputeHash($bytes)).ToLowerInvariant()
    }
    finally {
        $sha256.Dispose()
    }
    return Join-Path $defaultTargetRoot "cuda-release-$($hash.Substring(0, 12))"
}

function Assert-CudaExecutableArchitectures {
    param([Parameter(Mandatory)][string]$Path)

    $cuobjdump = Resolve-CudaTool "cuobjdump.exe"
    $cubinOutput = (& $cuobjdump -lelf $Path 2>&1 | Out-String)
    if ($LASTEXITCODE -ne 0) {
        throw "cuobjdump failed to inspect CUDA cubins in $Path"
    }
    foreach ($architecture in $requiredCudaArchitectures) {
        if ($cubinOutput -notmatch [regex]::Escape($architecture)) {
            throw "CUDA release is missing required $architecture cubins"
        }
    }

    $ptxOutput = (& $cuobjdump -lptx $Path 2>&1 | Out-String)
    if ($LASTEXITCODE -ne 0 -or $ptxOutput -match 'No PTX file found' -or $ptxOutput -notmatch 'sm_89') {
        throw "CUDA release is missing the required compute_89 PTX fallback"
    }
    Write-Host "Verified CUDA cubins for $($requiredCudaArchitectures -join ', ') and compute_89 PTX"
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
        [string[]]$Features = @(),
        [string]$FileSuffix = ""
    )

    $isCudaBuild = $Features -contains "cuda"
    $buildTargetRoot = if ($isCudaBuild) { Get-CudaReleaseTargetRoot } else { $defaultTargetRoot }
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

    $previousCudaArchitectures = [Environment]::GetEnvironmentVariable("CUDAARCHS", "Process")
    $previousCmakeToolchainFile = [Environment]::GetEnvironmentVariable("CMAKE_TOOLCHAIN_FILE", "Process")
    $previousCargoTargetDir = [Environment]::GetEnvironmentVariable("CARGO_TARGET_DIR", "Process")
    $cudaToolchainFile = $null
    try {
        $env:CARGO_TARGET_DIR = $buildTargetRoot
        if ($isCudaBuild) {
            $env:CUDAARCHS = $cudaArchitectures
            # ggml selects its native GPU before CMake initializes CUDAARCHS. An early
            # toolchain assignment overrides that default and makes the release portable.
            $cudaToolchainFile = Join-Path ([System.IO.Path]::GetTempPath()) "vrcs-cuda-release-$PID.cmake"
            $toolchainContents = "set(CMAKE_CUDA_ARCHITECTURES `"$cudaArchitectures`" CACHE STRING `"VRCS release CUDA architectures`" FORCE)"
            [System.IO.File]::WriteAllText($cudaToolchainFile, $toolchainContents, [System.Text.UTF8Encoding]::new($false))
            $env:CMAKE_TOOLCHAIN_FILE = $cudaToolchainFile
        }

        if (Test-Path -LiteralPath $bundleRoot) {
            Remove-Item -LiteralPath $bundleRoot -Recurse -Force
        }
        New-Item -ItemType Directory -Path $bundleRoot -Force | Out-Null

        Write-Host "Building $Label release"
        & npm @arguments | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "$Label Tauri NSIS build failed" }
    }
    finally {
        [Environment]::SetEnvironmentVariable("CUDAARCHS", $previousCudaArchitectures, "Process")
        [Environment]::SetEnvironmentVariable("CMAKE_TOOLCHAIN_FILE", $previousCmakeToolchainFile, "Process")
        [Environment]::SetEnvironmentVariable("CARGO_TARGET_DIR", $previousCargoTargetDir, "Process")
        if ($cudaToolchainFile -and (Test-Path -LiteralPath $cudaToolchainFile -PathType Leaf)) {
            Remove-Item -LiteralPath $cudaToolchainFile -Force
        }
    }

    $desktopExecutable = Join-Path $buildTargetRoot "release\vrcs-desktop.exe"
    if (-not (Test-Path -LiteralPath $desktopExecutable -PathType Leaf)) {
        throw "Built desktop executable not found: $desktopExecutable"
    }
    if ($isCudaBuild) {
        Assert-CudaExecutableArchitectures $desktopExecutable
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
    $artifactName = "VRCS-$Version-windows-x64$FileSuffix.exe"
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

Push-Location $repoRoot
try {
    Write-ReleaseTauriConfig `
        -TemplatePath $tauriReleaseConfigTemplatePath `
        -DestinationPath $generatedTauriConfigPath `
        -PublicKey $env:TAURI_UPDATER_PUBLIC_KEY

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

    if ($IncludeCuda) {
        Assert-Cuda13Toolchain
    }

    $releaseBuilds = @(Invoke-ReleaseBuild -Label "standard")
    if ($IncludeCuda) {
        $releaseBuilds += Invoke-ReleaseBuild -Label "CUDA" -Features @("cuda") -FileSuffix "-CUDA"
    }

    $platforms = [ordered]@{}
    foreach ($build in $releaseBuilds) {
        $variant = if ($build.ArtifactName.EndsWith("-CUDA.exe")) { "cuda" } else { "standard" }
        $platforms["windows-x86_64-$variant"] = [ordered]@{
            url = "https://github.com/Dreaminko/VRCS/releases/download/$Version/$($build.ArtifactName)"
            signature = Get-Content -LiteralPath $build.SignaturePath -Raw
        }
    }

    $latestPath = Join-Path $artifactRoot "latest.json"
    $latestJson = [ordered]@{
        version = $Version
        notes = "Windows 10/11 x64 release for VRCS $Version."
        pub_date = [DateTime]::UtcNow.ToString("o")
        platforms = $platforms
    } | ConvertTo-Json -Depth 4
    $utf8WithoutBom = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($latestPath, $latestJson, $utf8WithoutBom)
    Assert-NonEmptyFile $latestPath

    $releaseArtifacts = @($releaseBuilds | ForEach-Object {
        $_.ArtifactPath
        $_.SignaturePath
        $_.ChecksumPath
    }) + $latestPath
}
finally {
    if (Test-Path -LiteralPath $generatedTauriConfigPath) {
        Remove-Item -LiteralPath $generatedTauriConfigPath -Force
    }
    Pop-Location
}

Write-Host "Release packages ready"
foreach ($artifact in $releaseArtifacts) {
    Write-Host "  $artifact"
}
