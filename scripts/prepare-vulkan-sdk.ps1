[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
# Node can pass module paths from a different PowerShell version.
$env:PSModulePath = "$PSHOME\Modules;$env:PSModulePath"
$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$version = "1.4.309.0"
$sdkRoot = Join-Path $repoRoot "core\.cache\vulkan-sdk\$version"
if ($env:VULKAN_SDK -and (Test-Path -LiteralPath "$env:VULKAN_SDK\Bin\glslc.exe")) {
    $sdkRoot = $env:VULKAN_SDK
}
if (-not (Test-Path -LiteralPath "$sdkRoot\Bin\glslc.exe")) {
    $installerPath = Join-Path $repoRoot "core\.cache\VulkanSDK-$version.exe"
    New-Item -ItemType Directory -Path (Split-Path $installerPath) -Force | Out-Null
    if (-not (Test-Path -LiteralPath $installerPath)) {
        Invoke-WebRequest -Uri "https://sdk.lunarg.com/sdk/download/$version/windows/vulkan-sdk.exe" -OutFile $installerPath
    }
    if ((Get-FileHash -LiteralPath $installerPath -Algorithm SHA256).Hash.ToLowerInvariant() -ne "48b132169b64fe65cdb0f20970195335a65354e73f1ea5373032c2a8bbad4297") {
        throw "Vulkan SDK installer failed SHA-256 verification"
    }
    # Copy files into the workspace without changing the registry or system PATH.
    $installer = Start-Process -FilePath $installerPath -ArgumentList @("--root", "`"$sdkRoot`"", "--accept-licenses", "--default-answer", "--confirm-command", "install", "copy_only=1") -Wait -PassThru -WindowStyle Hidden
    if ($installer.ExitCode -ne 0) { throw "Vulkan SDK preparation failed ($($installer.ExitCode))" }
}
$env:VULKAN_SDK = $sdkRoot
$env:PATH = "$sdkRoot\Bin;$env:PATH"
# MSBuild file tracking exceeds MAX_PATH in GGML's nested shader build.
# Ninja avoids those tracking files and retains incremental native builds.
if (-not (Get-Command ninja -ErrorAction SilentlyContinue)) {
    throw "Ninja is required; install the Visual Studio CMake tools or add Ninja to PATH"
}
$env:CMAKE_GENERATOR = "Ninja"
& (Join-Path $PSScriptRoot "prepare-vulkan-runtime.ps1")
