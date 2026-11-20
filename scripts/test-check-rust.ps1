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
$script:failCallNumber = 0

function cargo {
    $cargoCalls.Add(($args -join " "))
    $global:LASTEXITCODE = if ($args[0] -eq $failCommand -or $cargoCalls.Count -eq $failCallNumber) { 17 } else { 0 }
}

function Assert-Calls {
    param(
        [hashtable]$Arguments,
        [string[]]$ExpectedCalls,
        [int]$ExpectedRuntimeCalls = 0
    )

    $script:cargoCalls.Clear()
    $runtimeCallsBefore = $global:vrcsRuntimeCalls
    & $checkScript @Arguments
    if (($script:cargoCalls -join "`n") -cne ($ExpectedCalls -join "`n")) {
        throw "Unexpected cargo calls:`nActual:`n$($script:cargoCalls -join "`n")`nExpected:`n$($ExpectedCalls -join "`n")"
    }
    if ($global:vrcsRuntimeCalls - $runtimeCallsBefore -ne $ExpectedRuntimeCalls) {
        throw "Unexpected Vulkan runtime preparation count"
    }
}

try {
    $fmtCall = "fmt --all -- --check"
    $cpuClippyCalls = @(
        "clippy --workspace --all-targets --locked -- -D warnings",
        "clippy -p vrcs-desktop --no-deps --all-targets --locked --features vr-preview -- -D warnings"
    )
    $cpuTestCalls = @("test --workspace --locked")
    $vulkanClippyCoreDesktop = "clippy -p vrcs-core -p vrcs-desktop --all-targets --locked --features vulkan -- -D warnings"
    $vulkanClippyScan = "clippy -p player-audio-scan -p player-audio-helper --all-targets --locked -- -D warnings"
    $ciVulkanTestCalls = @("test -p vrcs-core --locked --features vulkan asr::qwen_runtime::tests::")

    Assert-Calls -Arguments @{ Check = "format"; Vulkan = $true } -ExpectedCalls @($fmtCall)
    Assert-Calls -Arguments @{ Check = "clippy" } -ExpectedCalls $cpuClippyCalls
    Assert-Calls -Arguments @{ Check = "test" } -ExpectedCalls $cpuTestCalls
    Assert-Calls -Arguments @{ Check = "clippy"; Vulkan = $true } -ExpectedCalls @($vulkanClippyCoreDesktop, $vulkanClippyScan) -ExpectedRuntimeCalls 1
    Assert-Calls -Arguments @{ Check = "test"; Vulkan = $true } -ExpectedCalls @(
        "test -p vrcs-core -p vrcs-desktop --locked --features vulkan",
        "test -p player-audio-scan -p player-audio-helper --locked"
    ) -ExpectedRuntimeCalls 1

    Assert-Calls -Arguments @{ Check = "format"; Vulkan = $true; CiVulkanOnly = $true } -ExpectedCalls @($fmtCall)
    Assert-Calls -Arguments @{ Check = "clippy"; Vulkan = $true; CiVulkanOnly = $true } -ExpectedCalls @($vulkanClippyCoreDesktop) -ExpectedRuntimeCalls 1
    Assert-Calls -Arguments @{ Check = "test"; Vulkan = $true; CiVulkanOnly = $true } -ExpectedCalls $ciVulkanTestCalls -ExpectedRuntimeCalls 1
    Assert-Calls -Arguments @{ Check = "all"; Vulkan = $true; CiVulkanOnly = $true } -ExpectedCalls (@($fmtCall, $vulkanClippyCoreDesktop) + $ciVulkanTestCalls) -ExpectedRuntimeCalls 1
    Assert-Calls -Arguments @{} -ExpectedCalls (@($fmtCall) + $cpuClippyCalls + $cpuTestCalls)

    $script:cargoCalls.Clear()
    $caughtMissingVulkan = $false
    try { & $checkScript -Check test -CiVulkanOnly }
    catch {
        if ($_.Exception.Message -notlike "*CiVulkanOnly requires -Vulkan*") { throw }
        $caughtMissingVulkan = $true
    }
    if (-not $caughtMissingVulkan -or $script:cargoCalls.Count -ne 0) {
        throw "CI Vulkan scope must reject CPU checks before running Cargo"
    }

    Push-Location $PSScriptRoot
    try {
        foreach ($failure in @("fmt", "clippy", "test")) {
            $script:cargoCalls.Clear()
            $script:failCommand = $failure
            $expectedCounts = @{ fmt = 1; clippy = 2; test = 4 }
            $locationBefore = (Get-Location).Path
            $caughtFailure = $false
            try { & $checkScript }
            catch {
                if ($_.Exception.Message -notlike "*failed with exit code 17*") { throw }
                $caughtFailure = $true
            }
            if (-not $caughtFailure -or $script:cargoCalls.Count -ne $expectedCounts[$failure]) {
                throw "A failed $failure command must stop subsequent checks"
            }
            if ((Get-Location).Path -ne $locationBefore) { throw "Checks must restore the caller's directory" }
        }

        $script:failCommand = $null
        for ($failedCall = 2; $failedCall -le 3; $failedCall++) {
            $script:cargoCalls.Clear()
            $script:failCallNumber = $failedCall
            $caughtFailure = $false
            $locationBefore = (Get-Location).Path
            try { & $checkScript -Vulkan -CiVulkanOnly }
            catch {
                if ($_.Exception.Message -notlike "*failed with exit code 17*") { throw }
                $caughtFailure = $true
            }
            if (-not $caughtFailure -or $script:cargoCalls.Count -ne $failedCall) {
                throw "A failed CI Vulkan command $failedCall must propagate its failure"
            }
            if ((Get-Location).Path -ne $locationBefore) { throw "CI checks must restore the caller's directory" }
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
