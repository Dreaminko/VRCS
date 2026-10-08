[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
# Node can pass module paths from a different PowerShell version.
$env:PSModulePath = "$PSHOME\Modules;$env:PSModulePath"
$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$version = "1.4.309.0"
$resourceRoot = Join-Path $repoRoot "apps\desktop\src-tauri\resources\vulkan-runtime"
if (-not (Test-Path -LiteralPath "$resourceRoot\vulkan-1.dll") -or -not (Test-Path -LiteralPath "$resourceRoot\VulkanRT-License.txt")) {
    $cacheRoot = Join-Path $repoRoot "core\.cache"
    New-Item -ItemType Directory -Path $cacheRoot -Force | Out-Null
    $archive = Join-Path $cacheRoot "VulkanRT-$version-Components.zip"
    if (-not (Test-Path -LiteralPath $archive)) {
        Invoke-WebRequest -Uri "https://sdk.lunarg.com/sdk/download/$version/windows/VulkanRT-$version-Components.zip" -OutFile $archive
    }
    if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne "7d969f4d7b44e387667d3148f61559497c22d50cbe3d50adc9e5409afbce2df1") {
        throw "Vulkan runtime failed SHA-256 verification"
    }
    $extractRoot = Join-Path $cacheRoot "vulkan-runtime"
    Expand-Archive -LiteralPath $archive -DestinationPath $extractRoot -Force
    $components = Join-Path $extractRoot "VulkanRT-$version-Components"
    New-Item -ItemType Directory -Path $resourceRoot -Force | Out-Null
    Copy-Item -LiteralPath "$components\x64\vulkan-1.dll", "$components\VulkanRT-License.txt" -Destination $resourceRoot -Force
}
# Tests and release self-tests run before Tauri installs bundled resources.
$targetRoots = @("$repoRoot\core\target", "$repoRoot\apps\desktop\src-tauri\target")
if ($env:CARGO_TARGET_DIR) { $targetRoots += $env:CARGO_TARGET_DIR }
foreach ($targetRoot in $targetRoots) {
    foreach ($profile in @("debug", "release")) {
        foreach ($directory in @("$targetRoot\$profile", "$targetRoot\$profile\deps")) {
            New-Item -ItemType Directory -Path $directory -Force | Out-Null
            $destination = Join-Path $directory "vulkan-1.dll"
            if (-not (Test-Path -LiteralPath $destination) -or
                (Get-FileHash -LiteralPath $destination).Hash -ne (Get-FileHash -LiteralPath "$resourceRoot\vulkan-1.dll").Hash) {
                Copy-Item -LiteralPath "$resourceRoot\vulkan-1.dll" -Destination $directory -Force
            }
        }
    }
}
