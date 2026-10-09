[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$metadataScript = Join-Path $PSScriptRoot "write-release-metadata.ps1"
$testRoot = Join-Path ([System.IO.Path]::GetTempPath()) "vrcs-release-metadata-$([Guid]::NewGuid())"
New-Item -ItemType Directory -Path $testRoot | Out-Null
try {
    $version = "1.2.3"
    $installerName = "VRCS-$version-windows-x64.exe"
    $installer = Join-Path $testRoot $installerName
    [System.IO.File]::WriteAllText($installer, "test installer")
    [System.IO.File]::WriteAllText("$installer.sig", "test signature`n")
    $hash = (Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash.ToLowerInvariant()
    [System.IO.File]::WriteAllText("$installer.sha256", "$hash  $installerName")
    $latestPath = & $metadataScript -Version $version -ArtifactRoot $testRoot
    $latest = Get-Content -LiteralPath $latestPath -Raw | ConvertFrom-Json
    $targets = @($latest.platforms.PSObject.Properties.Name)
    if ($latest.version -ne $version -or $targets.Count -ne 2) { throw "Expected two update targets for this version" }
    $standard = $latest.platforms.'windows-x86_64-standard'
    $legacy = $latest.platforms.'windows-x86_64-cuda'
    $expectedUrl = "https://github.com/Dreaminko/VRCS/releases/download/$version/$installerName"
    if ($standard.url -ne $expectedUrl -or $standard.signature -ne "test signature") { throw "Standard target must use the signed installer" }
    if ($legacy.url -ne $standard.url -or $legacy.signature -ne $standard.signature) { throw "Legacy CUDA clients must receive the same signed standard installer" }

    function Assert-MetadataFailure {
        param([string]$ExpectedError)
        try { & $metadataScript -Version $version -ArtifactRoot $testRoot | Out-Null }
        catch {
            if ($_.Exception.Message -notlike "*$ExpectedError*") { throw }
            return
        }
        throw "Metadata generation must reject $ExpectedError"
    }

    [System.IO.File]::WriteAllText((Join-Path $testRoot "VRCS-$version-windows-x64-CUDA.exe"), "stale second installer")
    Assert-MetadataFailure "exactly one standard installer"
    Remove-Item -LiteralPath (Join-Path $testRoot "VRCS-$version-windows-x64-CUDA.exe")
    [System.IO.File]::WriteAllText("$installer.sig", " ")
    Assert-MetadataFailure "signature"
    [System.IO.File]::WriteAllText("$installer.sig", "test signature")
    [System.IO.File]::WriteAllText($installer, "modified installer")
    Assert-MetadataFailure "checksum"
}
finally {
    $resolvedTestRoot = [System.IO.Path]::GetFullPath($testRoot)
    $tempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
    if (-not $resolvedTestRoot.StartsWith($tempRoot, [System.StringComparison]::OrdinalIgnoreCase)) { throw "Test cleanup path is outside the temporary directory" }
    Remove-Item -LiteralPath $resolvedTestRoot -Recurse -Force
}
Write-Host "Release metadata tests passed"
