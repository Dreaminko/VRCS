[CmdletBinding()]
param(
    [switch]$SkipInstall
)

$ErrorActionPreference = "Stop"
$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$coreRoot = Join-Path $repoRoot "core-python"
$venvRoot = Join-Path $coreRoot ".venv-test"
$venvPython = Join-Path $venvRoot "Scripts\python.exe"
$repoPrefix = $repoRoot.TrimEnd('\') + '\'
$resolvedVenv = [System.IO.Path]::GetFullPath($venvRoot)

if (-not $resolvedVenv.StartsWith($repoPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "Refusing to modify a path outside the repository: $resolvedVenv"
}

if (-not (Test-Path -LiteralPath $venvPython)) {
    if ($SkipInstall) {
        throw "Missing test environment while SkipInstall is enabled: $venvRoot"
    }
    & py -3.12 -m venv $venvRoot
    if ($LASTEXITCODE -ne 0) { throw "Failed to create the Python 3.12 test environment" }
}

if (-not $SkipInstall) {
    & $venvPython -m pip install --upgrade pip
    if ($LASTEXITCODE -ne 0) { throw "Failed to upgrade pip in the test environment" }
    Push-Location $coreRoot
    try {
        & $venvPython -m pip install -e ".[dev]"
        if ($LASTEXITCODE -ne 0) { throw "Failed to install the VRCS Core test dependencies" }
    }
    finally {
        Pop-Location
    }
}

Push-Location $repoRoot
try {
    & $venvPython -m pytest "core-python\tests" -q
    if ($LASTEXITCODE -ne 0) { throw "Python tests failed" }
}
finally {
    Pop-Location
}
