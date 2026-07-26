[CmdletBinding()]
param(
    [string]$TargetTriple = "x86_64-pc-windows-msvc",
    [switch]$SkipInstall,
    [switch]$CleanEnvironment
)

$ErrorActionPreference = "Stop"
$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$coreRoot = Join-Path $repoRoot "core-python"
$venvRoot = Join-Path $coreRoot ".venv-release"
$venvPython = Join-Path $venvRoot "Scripts\python.exe"
$distRoot = Join-Path $coreRoot "dist-release"
$workRoot = Join-Path $coreRoot "build-release"
$tauriRoot = Join-Path $repoRoot "apps\desktop\src-tauri"
$binaryRoot = Join-Path $tauriRoot "binaries"
$resourceRoot = Join-Path $tauriRoot "resources"
$stagedExecutable = Join-Path $binaryRoot "vrcs-core-$TargetTriple.exe"
$stagedAudioHelper = Join-Path $binaryRoot "vrcs-process-audio-$TargetTriple.exe"
$stagedRuntime = Join-Path $resourceRoot "vrcs-core-runtime"
$audioHelperExecutable = Join-Path $tauriRoot "target\$TargetTriple\release\vrcs-process-audio.exe"

function Assert-WithinRepo([string]$Path) {
    $fullPath = [System.IO.Path]::GetFullPath($Path)
    $prefix = $repoRoot.TrimEnd('\') + '\'
    if (-not $fullPath.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to modify a path outside the repository: $fullPath"
    }
}

Assert-WithinRepo $venvRoot
Assert-WithinRepo $distRoot
Assert-WithinRepo $workRoot
Assert-WithinRepo $stagedExecutable
Assert-WithinRepo $stagedAudioHelper
Assert-WithinRepo $stagedRuntime
Assert-WithinRepo $audioHelperExecutable

if ($CleanEnvironment -and $SkipInstall) {
    throw "CleanEnvironment cannot be combined with SkipInstall"
}
if ($CleanEnvironment -and (Test-Path -LiteralPath $venvRoot)) {
    Remove-Item -LiteralPath $venvRoot -Recurse -Force
}

if (-not (Test-Path -LiteralPath $venvPython)) {
    & py -3.12 -m venv $venvRoot
    if ($LASTEXITCODE -ne 0) { throw "Failed to create the Python 3.12 release environment" }
}

if (-not $SkipInstall) {
    & $venvPython -m pip install --upgrade pip
    if ($LASTEXITCODE -ne 0) { throw "Failed to upgrade pip" }
    Push-Location $coreRoot
    try {
        & $venvPython -m pip install -e ".[full,release]"
        if ($LASTEXITCODE -ne 0) { throw "Failed to install the VRCS Core release dependencies" }
    }
    finally {
        Pop-Location
    }
}

& cargo build `
    --manifest-path (Join-Path $tauriRoot "Cargo.toml") `
    --release `
    --target $TargetTriple `
    --bin vrcs-process-audio
if ($LASTEXITCODE -ne 0) { throw "Failed to build the VRChat process audio helper" }
if (-not (Test-Path -LiteralPath $audioHelperExecutable)) {
    throw "Missing process audio helper: $audioHelperExecutable"
}

Push-Location $coreRoot
try {
    & $venvPython -m PyInstaller `
        --noconfirm `
        --clean `
        --distpath $distRoot `
        --workpath $workRoot `
        "vrcs-core.spec"
    if ($LASTEXITCODE -ne 0) { throw "PyInstaller failed to build VRCS Core" }
}
finally {
    Pop-Location
}

$builtRoot = Join-Path $distRoot "vrcs-core"
$builtExecutable = Join-Path $builtRoot "vrcs-core.exe"
$builtRuntime = Join-Path $builtRoot "vrcs-core-runtime"
if (-not (Test-Path -LiteralPath $builtExecutable)) {
    throw "Missing PyInstaller executable: $builtExecutable"
}
if (-not (Test-Path -LiteralPath $builtRuntime)) {
    throw "Missing PyInstaller runtime directory: $builtRuntime"
}

$selfTest = Start-Process `
    -FilePath $builtExecutable `
    -ArgumentList "--release-self-test" `
    -WorkingDirectory $builtRoot `
    -WindowStyle Hidden `
    -Wait `
    -PassThru
if ($selfTest.ExitCode -ne 0) {
    throw "VRCS Core release self-test failed with exit code $($selfTest.ExitCode)"
}

New-Item -ItemType Directory -Force -Path $binaryRoot | Out-Null
New-Item -ItemType Directory -Force -Path $resourceRoot | Out-Null
if (Test-Path -LiteralPath $stagedExecutable) {
    Remove-Item -LiteralPath $stagedExecutable -Force
}
if (Test-Path -LiteralPath $stagedAudioHelper) {
    Remove-Item -LiteralPath $stagedAudioHelper -Force
}
if (Test-Path -LiteralPath $stagedRuntime) {
    Remove-Item -LiteralPath $stagedRuntime -Recurse -Force
}
Copy-Item -LiteralPath $builtExecutable -Destination $stagedExecutable
Copy-Item -LiteralPath $audioHelperExecutable -Destination $stagedAudioHelper
Copy-Item -LiteralPath $builtRuntime -Destination $stagedRuntime -Recurse

Write-Host "Staged VRCS Core sidecar for $TargetTriple"
Write-Host "  Executable: $stagedExecutable"
Write-Host "  Audio helper: $stagedAudioHelper"
Write-Host "  Runtime:    $stagedRuntime"
