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

function Assert-CheckCommands {
    param([string]$Check, [string]$ExpectedCommand, [switch]$Vulkan)

    $script:cargoCalls.Clear()
    $runtimeCallsBefore = $global:vrcsRuntimeCalls
    & $checkScript -Check $Check -Vulkan:$Vulkan
    if ($script:cargoCalls.Count -ne 3) {
        throw "$Check must check all three crates exactly once"
    }
    $manifests = @("core/Cargo.toml", "apps/desktop/src-tauri/Cargo.toml", "crates/player-audio-scan/Cargo.toml")
    for ($i = 0; $i -lt $manifests.Count; $i++) {
        $manifest = $manifests[$i]
        $expectedCall = "$ExpectedCommand --manifest-path $manifest"
        if ($Check -eq "clippy") { $expectedCall += " --all-targets" }
        if ($Check -ne "format") { $expectedCall += " --locked" }
        if ($Vulkan -and $Check -ne "format" -and $i -lt 2) { $expectedCall += " --features vulkan" }
        if ($Check -eq "format") { $expectedCall += " -- --check" }
        if ($Check -eq "clippy") { $expectedCall += " -- -D warnings" }
        if ($script:cargoCalls[$i] -cne $expectedCall) {
            throw "$Check unexpectedly ran: $($script:cargoCalls[$i])"
        }
    }
    $expectedRuntimeCalls = if ($Vulkan -and $Check -ne "format") { 1 } else { 0 }
    if ($global:vrcsRuntimeCalls - $runtimeCallsBefore -ne $expectedRuntimeCalls) { throw "Unexpected Vulkan runtime preparation" }
}

function Assert-CiVulkanCommands {
    param([string]$Check, [string[]]$ExpectedCalls)

    $script:cargoCalls.Clear()
    $runtimeCallsBefore = $global:vrcsRuntimeCalls
    & $checkScript -Check $Check -Vulkan -CiVulkanOnly
    if (($script:cargoCalls -join "`n") -cne ($ExpectedCalls -join "`n")) {
        throw "Unexpected CI Vulkan $Check commands: $($script:cargoCalls -join '; ')"
    }
    $expectedRuntimeCalls = if ($Check -eq "format") { 0 } else { 1 }
    if ($global:vrcsRuntimeCalls - $runtimeCallsBefore -ne $expectedRuntimeCalls) {
        throw "Unexpected CI Vulkan runtime preparation"
    }
}

try {
    Assert-CheckCommands -Check format -ExpectedCommand fmt -Vulkan
    Assert-CheckCommands -Check clippy -ExpectedCommand clippy
    Assert-CheckCommands -Check test -ExpectedCommand test
    Assert-CheckCommands -Check clippy -ExpectedCommand clippy -Vulkan
    Assert-CheckCommands -Check test -ExpectedCommand test -Vulkan

    Assert-CiVulkanCommands -Check format -ExpectedCalls @(
        "fmt --manifest-path core/Cargo.toml -- --check",
        "fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check",
        "fmt --manifest-path crates/player-audio-scan/Cargo.toml -- --check"
    )
    Assert-CiVulkanCommands -Check clippy -ExpectedCalls @(
        "clippy --manifest-path core/Cargo.toml --all-targets --locked --features vulkan -- -D warnings",
        "clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets --locked --features vulkan -- -D warnings"
    )
    $ciVulkanTestCalls = @(
        "test --manifest-path core/Cargo.toml --all-targets --no-run --locked --features vulkan",
        "test --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets --no-run --locked --features vulkan",
        "test --manifest-path core/Cargo.toml --locked --features vulkan asr::qwen_runtime::tests::"
    )
    Assert-CiVulkanCommands -Check test -ExpectedCalls $ciVulkanTestCalls
    $ciVulkanAllCalls = @(
        "fmt --manifest-path core/Cargo.toml -- --check",
        "fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check",
        "fmt --manifest-path crates/player-audio-scan/Cargo.toml -- --check",
        "clippy --manifest-path core/Cargo.toml --all-targets --locked --features vulkan -- -D warnings",
        "clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets --locked --features vulkan -- -D warnings"
    ) + $ciVulkanTestCalls
    Assert-CiVulkanCommands -Check all -ExpectedCalls $ciVulkanAllCalls

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
    if ($script:cargoCalls.Count -ne 9) { throw "Default checks must run all three stages for all three crates" }

    Push-Location $PSScriptRoot
    try {
        foreach ($failure in @("fmt", "clippy", "test")) {
            $script:cargoCalls.Clear()
            $script:failCommand = $failure
            $expectedCalls = @{ fmt = 1; clippy = 4; test = 7 }
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

        foreach ($failure in @("clippy", "test")) {
            $script:cargoCalls.Clear()
            $script:failCommand = $failure
            $locationBefore = (Get-Location).Path
            $caughtFailure = $false
            try { & $checkScript -Check $failure -Vulkan -CiVulkanOnly }
            catch {
                if ($_.Exception.Message -notlike "*failed with exit code 17*") { throw }
                $caughtFailure = $true
            }
            if (-not $caughtFailure -or $script:cargoCalls.Count -ne 1) {
                throw "A failed CI Vulkan $failure command must stop subsequent checks"
            }
            if ((Get-Location).Path -ne $locationBefore) { throw "CI checks must restore the caller's directory" }
        }

        $script:failCommand = $null
        for ($failedCall = 2; $failedCall -le 3; $failedCall++) {
            $script:cargoCalls.Clear()
            $script:failCallNumber = $failedCall
            $caughtFailure = $false
            $locationBefore = (Get-Location).Path
            try { & $checkScript -Check test -Vulkan -CiVulkanOnly }
            catch {
                if ($_.Exception.Message -notlike "*failed with exit code 17*") { throw }
                $caughtFailure = $true
            }
            if (-not $caughtFailure -or $script:cargoCalls.Count -ne $failedCall) {
                throw "A failed CI Vulkan test command $failedCall must propagate its failure"
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
