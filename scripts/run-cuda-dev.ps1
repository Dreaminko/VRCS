[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidateSet("core", "desktop")]
    [string]$Target
)

$ErrorActionPreference = "Stop"
if (-not $env:CUDA_PATH) {
    throw "CUDA_PATH is not set. Install the NVIDIA CUDA Toolkit first."
}

$cudaBin = @(
    (Join-Path $env:CUDA_PATH "bin\x64"),
    (Join-Path $env:CUDA_PATH "bin")
) | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
if (-not $cudaBin) {
    throw "No CUDA runtime directory was found below $env:CUDA_PATH"
}

$env:PATH = "$cudaBin;$env:PATH"
$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
Push-Location $repoRoot
try {
    if ($Target -eq "core") {
        & cargo run --manifest-path core\Cargo.toml --features cuda
    }
    else {
        & npm --workspace apps/desktop run tauri -- dev --features cuda
    }
    if ($LASTEXITCODE -ne 0) {
        throw "CUDA $Target development command failed"
    }
}
finally {
    Pop-Location
}
