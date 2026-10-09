[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidatePattern('^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$')][string]$Version,
    [Parameter(Mandatory)][string]$ArtifactRoot
)

$ErrorActionPreference = "Stop"
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
    url = "https://github.com/Dreaminko/VRCS/releases/download/$Version/$artifactName"
    signature = $signature.Trim()
}
$latest = [ordered]@{
    version = $Version
    notes = "Windows 10/11 x64 release for VRCS $Version. Local Whisper selections migrate to managed Qwen. Cloud failures reconnect without Whisper fallback. Existing Whisper files remain on disk and can be removed manually when no longer needed by older versions."
    pub_date = [DateTime]::UtcNow.ToString("o")
    platforms = [ordered]@{
        'windows-x86_64-standard' = $platform
        # Older CUDA clients request this target. Keep the same signed upgrade.
        'windows-x86_64-cuda' = $platform
    }
}
$latestPath = Join-Path $ArtifactRoot "latest.json"
[System.IO.File]::WriteAllText($latestPath, ($latest | ConvertTo-Json -Depth 4), [System.Text.UTF8Encoding]::new($false))
return $latestPath
