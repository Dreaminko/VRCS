[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$testRoot = Join-Path ([System.IO.Path]::GetTempPath()) "vrcs-check-rust-$([Guid]::NewGuid())"
$testScripts = Join-Path $testRoot "scripts"
New-Item -ItemType Directory -Path $testScripts | Out-Null
$checkScript = Join-Path $testScripts "check-rust.ps1"
Copy-Item -LiteralPath (Join-Path $PSScriptRoot "check-rust.ps1") -Destination $checkScript
# A test copy makes accidental SDK preparation fail without network access.
[System.IO.File]::WriteAllText((Join-Path $testScripts "prepare-vulkan-runtime.ps1"), '$global:vrcsRuntimeCalls++')
$global:vrcsRuntimeCalls = 0
$script:cargoCalls = [System.Collections.Generic.List[string]]::new()
$script:failCommand = $null

function cargo {
    $cargoCalls.Add(($args -join " "))
    $global:LASTEXITCODE = if ($args[0] -eq $failCommand) { 17 } else { 0 }
}

function Assert-CheckCommands {
    param([string]$Check, [string]$ExpectedCommand, [switch]$Vulkan)

    $script:cargoCalls.Clear()
    $runtimeCallsBefore = $global:vrcsRuntimeCalls
    & $checkScript -Check $Check -Vulkan:$Vulkan
    if ($script:cargoCalls.Count -ne 2) {
        throw "$Check must check both crates exactly once"
    }
    foreach ($call in $script:cargoCalls) {
        if (-not $call.StartsWith("$ExpectedCommand ")) {
            throw "$Check unexpectedly ran: $call"
        }
        if ($Check -ne "format" -and -not $call.Contains("--locked")) {
            throw "$Check must use the lockfile"
        }
        $usesVulkan = $call.Contains("--features vulkan")
        if ($usesVulkan -ne ($Vulkan -and $Check -ne "format")) {
            throw "$Check must select only the requested CPU or Vulkan features"
        }
    }
    $expectedRuntimeCalls = if ($Vulkan -and $Check -ne "format") { 1 } else { 0 }
    if ($global:vrcsRuntimeCalls - $runtimeCallsBefore -ne $expectedRuntimeCalls) { throw "Unexpected Vulkan runtime preparation" }
}

try {
    Assert-CheckCommands -Check format -ExpectedCommand fmt -Vulkan
    Assert-CheckCommands -Check clippy -ExpectedCommand clippy
    Assert-CheckCommands -Check test -ExpectedCommand test
    Assert-CheckCommands -Check clippy -ExpectedCommand clippy -Vulkan
    Assert-CheckCommands -Check test -ExpectedCommand test -Vulkan

    $script:cargoCalls.Clear()
    try {
        & $checkScript -Cuda
        throw "Removed CUDA switch must fail"
    }
    catch [System.Management.Automation.ParameterBindingException] {
        if ($script:cargoCalls.Count -ne 0) { throw "Removed CUDA switch must not run Cargo" }
    }

    $script:cargoCalls.Clear()
    & $checkScript
    if ($script:cargoCalls.Count -ne 6) { throw "Default checks must run all three stages for both crates" }

    Push-Location $PSScriptRoot
    try {
        foreach ($failure in @("fmt", "clippy", "test")) {
            $script:cargoCalls.Clear()
            $script:failCommand = $failure
            $expectedCalls = @{ fmt = 1; clippy = 3; test = 5 }
            $locationBefore = (Get-Location).Path
            $caughtFailure = $false
            try {
                & $checkScript
            }
            catch {
                if ($_.Exception.Message -notlike "*failed with exit code 17*") { throw }
                $caughtFailure = $true
            }
            if (-not $caughtFailure -or $script:cargoCalls.Count -ne $expectedCalls[$failure]) {
                throw "A failed $failure command must stop subsequent checks"
            }
            if ((Get-Location).Path -ne $locationBefore) { throw "Checks must restore the caller's directory" }
        }
    }
    finally {
        Pop-Location
    }
}
finally {
    $resolvedTestRoot = [System.IO.Path]::GetFullPath($testRoot)
    $tempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
    if (-not $resolvedTestRoot.StartsWith($tempRoot, [System.StringComparison]::OrdinalIgnoreCase)) { throw "Test cleanup path is outside the temporary directory" }
    Remove-Item -LiteralPath $resolvedTestRoot -Recurse -Force
    Remove-Variable -Name vrcsRuntimeCalls -Scope Global
}

# Failure injection must not leave a nonzero native exit code for CI's shell wrapper.
$global:LASTEXITCODE = 0
Write-Host "Rust check script tests passed"
