[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidatePattern('^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$')][string]$Version,
    [Parameter(Mandatory)][string]$ArtifactRoot
)

$ErrorActionPreference = "Stop"
# A build job only describes its own package. The publish job is the sole
# writer of latest.json after every native architecture has been validated.
$result = & node (Join-Path $PSScriptRoot "release-metadata.mjs") target $ArtifactRoot $Version windows x86_64 nsis
if ($LASTEXITCODE -ne 0) { throw "Windows target manifest generation failed" }
return $result
