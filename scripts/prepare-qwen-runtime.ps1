[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
# Node can pass module paths from a different PowerShell version.
$env:PSModulePath = "$PSHOME\Modules;$env:PSModulePath"
$release = "b11501"
$commit = "46baf1f1fec5a06d1e52122a9207ca978b720f95"
$checksum = "6bf677025ae9ccdf5ca5e1ee1961ecaf25759f8cf63e875925a83bfaf00b7475"
$archive = Join-Path ([System.IO.Path]::GetTempPath()) "vrcs-llama-$release-vulkan.zip"
$destination = Join-Path $PSScriptRoot "..\apps\desktop\src-tauri\resources\qwen-runtime"
$marker = Join-Path $destination "version.txt"
if ((Test-Path -LiteralPath $marker) -and (Get-Content -LiteralPath $marker -Raw).Trim() -eq $checksum) {
    $required = @("llama-server.exe", "llama-server-impl.dll", "llama.dll", "llama-common.dll", "mtmd.dll",
        "ggml.dll", "ggml-base.dll", "ggml-vulkan.dll", "ggml-cpu-x64.dll", "libomp.dll", "LICENSE", "LICENSE-LLVM-OpenMP")
    if (@($required | Where-Object { -not (Test-Path -LiteralPath (Join-Path $destination $_)) }).Count -eq 0) { return }
}

if (-not (Test-Path -LiteralPath $archive) -or (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $checksum) {
    Write-Host "Downloading Qwen ASR runtime ($release, Vulkan + CPU)"
    Invoke-WebRequest -Uri "https://github.com/ggml-org/llama.cpp/releases/download/$release/llama-$release-bin-win-vulkan-x64.zip" -OutFile $archive
}
if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $checksum) {
    throw "Qwen ASR runtime archive failed SHA256 verification"
}

New-Item -ItemType Directory -Path $destination -Force | Out-Null
Add-Type -AssemblyName System.IO.Compression.FileSystem
$zip = [System.IO.Compression.ZipFile]::OpenRead($archive)
try {
    foreach ($entry in $zip.Entries) {
        $name = $entry.Name
        if ($name -eq "llama-server.exe" -or $name -eq "LICENSE-LLVM-OpenMP" -or
            $name -match '^(ggml.*|llama|llama-common|llama-server-impl|mtmd|libomp)\.dll$') {
            [System.IO.Compression.ZipFileExtensions]::ExtractToFile($entry, (Join-Path $destination $name), $true)
        }
    }
}
finally { $zip.Dispose() }
Invoke-WebRequest -Uri "https://raw.githubusercontent.com/ggml-org/llama.cpp/$commit/LICENSE" -OutFile (Join-Path $destination "LICENSE")
Set-Content -LiteralPath $marker -Value $checksum -Encoding ascii
Write-Host "Qwen ASR runtime ready: $release"
