[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
& node --test (Join-Path $PSScriptRoot "test-release-metadata.mjs")
if ($LASTEXITCODE -ne 0) { throw "Release metadata consumer tests failed" }
