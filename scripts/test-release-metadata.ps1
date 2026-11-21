[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$metadataScript = Join-Path $PSScriptRoot "write-release-metadata.ps1"
$testRoot = Join-Path ([System.IO.Path]::GetTempPath()) "vrcs-release-metadata-$([Guid]::NewGuid())"
New-Item -ItemType Directory -Path $testRoot | Out-Null
try {
    function New-TestPeInstaller {
        param([string]$Path)
        $bytes = [byte[]]::new(128)
        $bytes[0] = [byte][char]'M'
        $bytes[1] = [byte][char]'Z'
        [System.BitConverter]::GetBytes([uint32]64).CopyTo($bytes, 60)
        $bytes[64] = [byte][char]'P'
        $bytes[65] = [byte][char]'E'
        $bytes[66] = 0
        $bytes[67] = 0
        [System.BitConverter]::GetBytes([uint16]0x14c).CopyTo($bytes, 68)
        [System.IO.File]::WriteAllBytes($Path, $bytes)
    }

    function Assert-MetadataFailure {
        param(
            [string]$ExpectedError,
            [hashtable]$ExtraArguments = @{},
            [string]$TargetVersion = $version
        )
        try { & $metadataScript -Version $TargetVersion -ArtifactRoot $testRoot @ExtraArguments | Out-Null }
        catch {
            if ($_.Exception.Message -notlike "*$ExpectedError*") { throw }
            return
        }
        throw "Metadata generation must reject $ExpectedError"
    }

    # Stable target manifest path (delegates to release-metadata.mjs target without squashfs-tools).
    $version = "1.2.3"
    $installerName = "VRCS-$version-windows-x64.exe"
    $installer = Join-Path $testRoot $installerName
    New-TestPeInstaller -Path $installer
    [System.IO.File]::WriteAllText("$installer.sig", "test signature`n")
    $manifestPath = (& $metadataScript -Version $version -ArtifactRoot $testRoot | Select-Object -Last 1).Trim()
    if ((Split-Path -Leaf $manifestPath) -cne "windows-x86_64-nsis.manifest.json") {
        throw "Expected windows-x86_64-nsis.manifest.json, got $manifestPath"
    }
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    $hash = (Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($manifest.version -ne $version -or
        $manifest.os -cne "windows" -or
        $manifest.arch -cne "x86_64" -or
        $manifest.channel -cne "nsis" -or
        $manifest.artifact -cne $installerName -or
        $manifest.sha256 -cne $hash -or
        $manifest.size -ne 128 -or
        $manifest.signature -cne "$installerName.sig") {
        throw "Windows target manifest did not match expected fields"
    }
    $checksum = (Get-Content -LiteralPath "$installer.sha256" -Raw).Trim()
    if ($checksum -cne "$hash  $installerName") {
        throw "Installer checksum file did not match expected digest"
    }

    [System.IO.File]::WriteAllText("$installer.sig", " ")
    Assert-MetadataFailure -ExpectedError "signature"
    [System.IO.File]::WriteAllText("$installer.sig", "test signature`n")
    [System.IO.File]::WriteAllText($installer, "not a PE installer")
    Assert-MetadataFailure -ExpectedError "PE installer"

    # Nightly path (self-contained inside write-release-metadata.ps1).
    $nightlyVersion = "1.2.4-nightly.20261119"
    $nightlyName = "VRCS-$nightlyVersion-windows-x64.exe"
    $nightlyInstaller = Join-Path $testRoot $nightlyName
    [System.IO.File]::WriteAllText($nightlyInstaller, "test nightly")
    [System.IO.File]::WriteAllText("$nightlyInstaller.sig", "nightly sig`n")
    $nightlyHash = (Get-FileHash -LiteralPath $nightlyInstaller -Algorithm SHA256).Hash.ToLowerInvariant()
    [System.IO.File]::WriteAllText("$nightlyInstaller.sha256", "$nightlyHash  $nightlyName")
    $sha = "a" * 40
    $nightlyPath = & $metadataScript -Version $nightlyVersion -ArtifactRoot $testRoot -Channel nightly -BuildDate "2026-11-19" -SourceSha $sha
    $nightly = Get-Content -LiteralPath $nightlyPath -Raw | ConvertFrom-Json
    $nightlyTargets = @($nightly.platforms.PSObject.Properties.Name)
    if ((Split-Path -Leaf $nightlyPath) -ne "nightly.json" -or
        $nightly.version -ne $nightlyVersion -or
        $nightly.build_date -ne "2026-11-19" -or
        $nightly.source_sha -ne $sha -or
        $nightlyTargets.Count -ne 1 -or
        $nightlyTargets[0] -cne "windows-x86_64-standard" -or
        $nightly.platforms.'windows-x86_64-standard'.url -ne "https://github.com/Dreaminko/VRCS/releases/download/nightly-20261119/$nightlyName" -or
        $nightly.platforms.'windows-x86_64-standard'.signature -cne "nightly sig") {
        throw "Nightly metadata did not match expected fields"
    }

    [System.IO.File]::WriteAllText((Join-Path $testRoot "VRCS-$nightlyVersion-windows-x64-extra.exe"), "stale second installer")
    Assert-MetadataFailure -ExpectedError "exactly one standard installer" -TargetVersion $nightlyVersion -ExtraArguments @{ Channel = "nightly"; BuildDate = "2026-11-19"; SourceSha = $sha }
    Remove-Item -LiteralPath (Join-Path $testRoot "VRCS-$nightlyVersion-windows-x64-extra.exe")
    [System.IO.File]::WriteAllText("$nightlyInstaller.sig", " ")
    Assert-MetadataFailure -ExpectedError "signature" -TargetVersion $nightlyVersion -ExtraArguments @{ Channel = "nightly"; BuildDate = "2026-11-19"; SourceSha = $sha }
    [System.IO.File]::WriteAllText("$nightlyInstaller.sig", "nightly sig")
    [System.IO.File]::WriteAllText($nightlyInstaller, "modified nightly")
    Assert-MetadataFailure -ExpectedError "checksum" -TargetVersion $nightlyVersion -ExtraArguments @{ Channel = "nightly"; BuildDate = "2026-11-19"; SourceSha = $sha }
    Assert-MetadataFailure -ExpectedError "Nightly metadata requires" -TargetVersion $nightlyVersion -ExtraArguments @{ Channel = "nightly"; BuildDate = "2026-11-18"; SourceSha = $sha }
}
finally {
    $resolvedTestRoot = [System.IO.Path]::GetFullPath($testRoot)
    $tempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
    if (-not $resolvedTestRoot.StartsWith($tempRoot, [System.StringComparison]::OrdinalIgnoreCase)) { throw "Test cleanup path is outside the temporary directory" }
    Remove-Item -LiteralPath $resolvedTestRoot -Recurse -Force
}
Write-Host "Release metadata tests passed"
