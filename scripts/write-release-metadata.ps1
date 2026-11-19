[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidatePattern('^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$')][string]$Version,
    [Parameter(Mandatory)][string]$ArtifactRoot,
    [ValidateSet('stable', 'nightly')][string]$Channel = 'stable',
    [string]$BuildDate,
    [string]$SourceSha
)

$ErrorActionPreference = "Stop"
if ($Channel -ne 'nightly') {
    # A build job only describes its own package. The publish job is the sole
    # writer of latest.json after every native architecture has been validated.
    $result = & node (Join-Path $PSScriptRoot "release-metadata.mjs") target $ArtifactRoot $Version windows x86_64 nsis
    if ($LASTEXITCODE -ne 0) { throw "Windows target manifest generation failed" }
    return $result
}

$dateSuffix = $BuildDate.Replace('-', '')
if ($BuildDate -cnotmatch '^\d{4}-\d{2}-\d{2}$' -or
    $SourceSha -cnotmatch '^[0-9a-f]{40}$' -or
    $Version -cnotmatch "^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)-nightly\.$dateSuffix$") {
    throw 'Nightly metadata requires a matching version, build date, and full source SHA'
}
$tag = "nightly-$dateSuffix"
$repository = 'Dreaminko/VRCS'
if ($env:GITHUB_REPOSITORY) { $repository = $env:GITHUB_REPOSITORY }
$metadataName = 'nightly.json'
$artifactName = "VRCS-$Version-windows-x64.exe"
$installers = @(Get-ChildItem -LiteralPath $ArtifactRoot -Filter "VRCS-$Version-windows-x64*.exe" -File)
if ($installers.Count -ne 1 -or $installers[0].Name -ne $artifactName -or $installers[0].Length -eq 0) {
    throw "Expected exactly one standard installer for $Version in $ArtifactRoot"
}
$installer = $installers[0]
$signaturePath = "$($installer.FullName).sig"
$signature = Get-Content -LiteralPath $signaturePath -Raw
if ([string]::IsNullOrWhiteSpace($signature)) { throw "Installer signature must not be empty" }
$hash = (Get-FileHash -LiteralPath $installer.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
$checksum = Get-Content -LiteralPath "$($installer.FullName).sha256" -Raw
if ($checksum.Trim() -cne "$hash  $artifactName") { throw "Installer checksum does not match the artifact" }

$platform = [ordered]@{
    url = "https://github.com/$repository/releases/download/$tag/$artifactName"
    signature = $signature.Trim()
}
$latest = [ordered]@{
    version = $Version
    notes = "Windows 10/11 x64 Nightly for VRCS $Version, built from the last main update on $BuildDate (Beijing time)."
    pub_date = [DateTime]::UtcNow.ToString("o")
    platforms = [ordered]@{
        'windows-x86_64-standard' = $platform
    }
    build_date = $BuildDate
    source_sha = $SourceSha
}
$metadataPath = Join-Path $ArtifactRoot $metadataName
[System.IO.File]::WriteAllText($metadataPath, ($latest | ConvertTo-Json -Depth 4), [System.Text.UTF8Encoding]::new($false))
return $metadataPath
