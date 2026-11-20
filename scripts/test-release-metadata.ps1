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
    if ($latest.version -ne $version -or $targets.Count -ne 1 -or $targets[0] -cne "windows-x86_64-standard") { throw "Expected only the standard update target for this version" }
    $standard = $latest.platforms.'windows-x86_64-standard'
    $expectedUrl = "https://github.com/Dreaminko/VRCS/releases/download/$version/$installerName"
    if ($standard.url -ne $expectedUrl -or $standard.signature -ne "test signature") { throw "Standard target must use the signed installer" }

    function Assert-MetadataFailure {
        param([string]$ExpectedError)
        try { & $metadataScript -Version $version -ArtifactRoot $testRoot | Out-Null }
        catch {
            if ($_.Exception.Message -notlike "*$ExpectedError*") { throw }
            return
        }
        throw "Metadata generation must reject $ExpectedError"
    }

    [System.IO.File]::WriteAllText((Join-Path $testRoot "VRCS-$version-windows-x64-extra.exe"), "stale second installer")
    Assert-MetadataFailure "exactly one standard installer"
    Remove-Item -LiteralPath (Join-Path $testRoot "VRCS-$version-windows-x64-extra.exe")
    [System.IO.File]::WriteAllText("$installer.sig", " ")
    Assert-MetadataFailure "signature"
    [System.IO.File]::WriteAllText("$installer.sig", "test signature")
    [System.IO.File]::WriteAllText($installer, "modified installer")
    Assert-MetadataFailure "checksum"

    $nightlyVersion = "1.2.4-nightly.20261119"
    $nightlyName = "VRCS-$nightlyVersion-windows-x64.exe"
    $nightlyInstaller = Join-Path $testRoot $nightlyName
    [System.IO.File]::WriteAllText($nightlyInstaller, "test nightly")
    [System.IO.File]::WriteAllText("$nightlyInstaller.sig", "nightly sig")
    $nightlyHash = (Get-FileHash -LiteralPath $nightlyInstaller -Algorithm SHA256).Hash.ToLowerInvariant()
    [System.IO.File]::WriteAllText("$nightlyInstaller.sha256", "$nightlyHash  $nightlyName")
    $sha = "a" * 40
    $nightlyPath = & $metadataScript -Version $nightlyVersion -ArtifactRoot $testRoot -Channel nightly -BuildDate "2026-11-19" -SourceSha $sha
    $nightly = Get-Content -LiteralPath $nightlyPath -Raw | ConvertFrom-Json
    $nightlyTargets = @($nightly.platforms.PSObject.Properties.Name)
    if ((Split-Path -Leaf $nightlyPath) -ne "nightly.json" -or $nightly.version -ne $nightlyVersion -or $nightly.build_date -ne "2026-11-19" -or $nightly.source_sha -ne $sha -or $nightlyTargets.Count -ne 1 -or $nightlyTargets[0] -cne "windows-x86_64-standard" -or $nightly.platforms.'windows-x86_64-standard'.url -ne "https://github.com/Dreaminko/VRCS/releases/download/nightly-20261119/$nightlyName") {
        throw "Nightly metadata did not match expected fields"
    }
    $caughtNightlyMismatch = $false
    try { & $metadataScript -Version $nightlyVersion -ArtifactRoot $testRoot -Channel nightly -BuildDate "2026-11-18" -SourceSha $sha | Out-Null }
    catch {
        if ($_.Exception.Message -notlike "*Nightly metadata requires*") { throw }
        $caughtNightlyMismatch = $true
    }
    if (-not $caughtNightlyMismatch) { throw "Nightly metadata must reject mismatched build date" }
}
finally {
    $resolvedTestRoot = [System.IO.Path]::GetFullPath($testRoot)
    $tempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
    if (-not $resolvedTestRoot.StartsWith($tempRoot, [System.StringComparison]::OrdinalIgnoreCase)) { throw "Test cleanup path is outside the temporary directory" }
    Remove-Item -LiteralPath $resolvedTestRoot -Recurse -Force
}
Write-Host "Release metadata tests passed"
