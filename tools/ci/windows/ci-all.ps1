#Requires -Version 5.1
# ci-all.ps1 - Local CI pipeline for fast development
# Modes: dev (fast), pre-push (medium), ci (full)
#
# Usage:
#   .\scripts\ci\windows\ci-all.ps1                    # dev: fmt + tests (~5s)
#   .\scripts\ci\windows\ci-all.ps1 -Mode pre-push     # + clippy + release builds (~30s)
#   .\scripts\ci\windows\ci-all.ps1 -Mode ci           # + Linux x64 + ARM64 (~10min)
#   .\scripts\ci\windows\ci-all.ps1 -Mode dev -Clean   # clean cache before running
#   .\scripts\ci\windows\ci-all.ps1 -Mode ci -SkipArm  # ci without ARM64
#   .\scripts\ci\windows\ci-all.ps1 -Mode ci -WslDistro Ubuntu -WslBuildDir /mnt/d/build

[CmdletBinding()]
param(
    [ValidateSet("dev","pre-push","ci")]
    [string]$Mode = "dev",
    [switch]$Clean,
    [switch]$SkipArm,
    [switch]$SkipLinux,
    [string]$WslDistro = "Debian",
    [string]$WslBuildDir = "/mnt/e/isi-music-build"
)

$root = Resolve-Path "$PSScriptRoot\..\..\.."
Set-Location $root

# Convert the Windows project path to a WSL path (C:\foo -> /mnt/c/foo)
$winRoot = (Resolve-Path "$PSScriptRoot\..\..\..").Path
$wslProject = "/mnt/" + $winRoot[0].ToString().ToLower() + $winRoot.Substring(2).Replace('\', '/')

# sccache only on Windows (not installed in WSL)
$sccache = Get-Command sccache -ErrorAction SilentlyContinue
if ($sccache) {
    $env:RUSTC_WRAPPER = $sccache.Source
    & $sccache.Source --start-server 2>&1 | Out-Null
}

# Console output helper
function Out-Msg {
    param([string]$Text, [string]$Color = "White")
    $colorMap = @{
        "White" = [ConsoleColor]::White; "Green" = [ConsoleColor]::Green
        "Red" = [ConsoleColor]::Red; "Yellow" = [ConsoleColor]::Yellow
        "Cyan" = [ConsoleColor]::Cyan; "DarkGray" = [ConsoleColor]::DarkGray
        "DarkRed" = [ConsoleColor]::DarkRed
    }
    $c = $colorMap[$Color]
    if ($null -eq $c) { $c = [ConsoleColor]::White }
    [Console]::ForegroundColor = $c
    [Console]::WriteLine($Text)
    [Console]::ResetColor()
}

# Pipeline state
$script:results = @()
$script:totalStart = Get-Date

function Run-Step {
    param(
        [string]$Platform,
        [string]$Label,
        [scriptblock]$Action,
        [bool]$Skip = $false
    )

    if ($Skip) {
        Out-Msg "  [SKIP] $Label" "DarkGray"
        $script:results += [pscustomobject]@{
            Platform = $Platform; Label = $Label; Status = "SKIP"; Time = 0
        }
        return
    }

    $start = Get-Date
    Out-Msg "  [RUN]  $Label" "Cyan"

    $output = & $Action 2>&1 | ForEach-Object { $_.ToString() }
    $code = $LASTEXITCODE
    if ($null -eq $code) { $code = 0 }
    $elapsed = ((Get-Date) - $start).TotalSeconds

    if ($code -ne 0) {
        $output | Select-Object -Last 5 | ForEach-Object {
            Out-Msg "      $_" "DarkRed"
        }
        Out-Msg "  [FAIL] $Label ($('{0:N1}' -f $elapsed)s)" "Red"
        $script:results += [pscustomobject]@{
            Platform = $Platform; Label = $Label; Status = "FAIL"; Time = [math]::Round($elapsed, 1)
        }
    } else {
        Out-Msg "  [PASS] $Label ($('{0:N1}' -f $elapsed)s)" "Green"
        $script:results += [pscustomobject]@{
            Platform = $Platform; Label = $Label; Status = "PASS"; Time = [math]::Round($elapsed, 1)
        }
    }
}

# Detect nextest
$hasNextest = $null -ne (Get-Command cargo-nextest -ErrorAction SilentlyContinue)
$testCmd = if ($hasNextest) { "cargo nextest run" } else { "cargo test" }

# Clean
if ($Clean) {
    Out-Msg "" "White"
    Out-Msg "-- Cleaning cache --" "Yellow"
    cargo clean 2>&1 | Out-Null
    if ($Mode -eq "ci") {
        wsl -d $WslDistro -- bash -lc "rm -rf '$WslBuildDir/target'" 2>&1 | Out-Null
    }
    Out-Msg "  cargo clean done" "Green"
}

# ====================================================================
# DEV MODE: fmt + test (fast, ~5s)
# ====================================================================
Out-Msg "" "White"
Out-Msg "-- Mode: $Mode --" "Yellow"

# fmt --check (always, fast)
Run-Step "win" "cargo fmt --check" {
    cargo fmt --check
}

# test (nextest when available, otherwise cargo test)
Run-Step "win" "$testCmd" {
    if ($hasNextest) { cargo nextest run --locked } else { cargo test --locked }
}

# ====================================================================
# PRE-PUSH MODE: + clippy + release builds
# ====================================================================
if ($Mode -eq "pre-push" -or $Mode -eq "ci") {
    Run-Step "win" "cargo clippy -- -D warnings" {
        cargo clippy --all-targets --all-features --locked -- -D warnings
    }

    Run-Step "win" "cargo build --release" {
        cargo build --release --locked
    }

    Run-Step "win" "cargo build --release -F mpris" {
        cargo build --release --locked -F mpris
    }

    Run-Step "win" "cargo build --release (minimal)" {
        cargo build --release --locked --no-default-features -F spotify,discord
    }
}

# ====================================================================
# CI MODE: + Linux x86_64 + ARM64 via WSL2
# ====================================================================
if ($Mode -eq "ci") {
    $cleanArg = if ($Clean) { "--clean" } else { "" }

    # Linux x86_64
    if (-not $SkipLinux) {
        Out-Msg "" "White"
        Out-Msg "-- Linux x86_64 (WSL2) --" "Yellow"

        Run-Step "linux-x64" "cargo build+test (x86_64-linux)" {
            wsl -d $WslDistro -- bash -lc "ISI_BUILD_DIR='$WslBuildDir' bash $wslProject/tools/ci/windows/ci-wsl.sh x64 $cleanArg 2>&1"
        }
    }

    # ARM64
    if (-not $SkipArm) {
        Out-Msg "" "White"
        Out-Msg "-- Linux ARM64 (WSL2 cross-compile) --" "Yellow"

        Run-Step "linux-arm64" "cargo build --release (aarch64-linux)" {
            wsl -d $WslDistro -- bash -lc "ISI_BUILD_DIR='$WslBuildDir' bash $wslProject/tools/ci/windows/ci-wsl.sh arm64 $cleanArg --build-only 2>&1"
        }
    }
}

# ====================================================================
# REPORT
# ====================================================================
$totalElapsed = ((Get-Date) - $script:totalStart).TotalSeconds
$pass = [int]@($script:results | Where-Object { $_.Status -eq "PASS" }).Count
$fail = [int]@($script:results | Where-Object { $_.Status -eq "FAIL" }).Count
$skipped = [int]@($script:results | Where-Object { $_.Status -eq "SKIP" }).Count

Out-Msg "" "White"
Out-Msg "===========================================================" "Yellow"
Out-Msg "  CI REPORT - isi_music [$Mode]" "Yellow"
Out-Msg "===========================================================" "Yellow"
Out-Msg "" "White"

$header = "  {0,-14} {1,-42} {2,-6} {3}" -f "Platform", "Step", "Status", "Time"
Out-Msg $header "DarkGray"
Out-Msg "  $('-' * 71)" "DarkGray"

foreach ($r in $script:results) {
    $color = switch ($r.Status) {
        "PASS" { "Green" }
        "FAIL" { "Red" }
        "SKIP" { "DarkGray" }
    }
    $label = $r.Label
    if ($label.Length -gt 42) { $label = $label.Substring(0, 39) + "..." }
    $line = "  {0,-14} {1,-42} {2,-6} {3}s" -f $r.Platform, $label, $r.Status, $r.Time
    Out-Msg $line $color
}

Out-Msg "  $('-' * 71)" "DarkGray"
Out-Msg "" "White"
Out-Msg "  PASS:     $pass" "Green"
if ($fail -gt 0) {
    Out-Msg "  FAIL:     $fail" "Red"
} else {
    Out-Msg "  FAIL:     $fail" "DarkGray"
}
Out-Msg "  SKIPPED:  $skipped" "DarkGray"
Out-Msg "  TOTAL:    $($script:results.Count)" "DarkGray"
Out-Msg "  TIME:     $('{0:N1}' -f $totalElapsed)s" "DarkGray"
Out-Msg "" "White"

if ($fail -gt 0) {
    Out-Msg "  RESULT: FAILED - fix errors before pushing" "Red"
    Out-Msg "" "White"
    $script:results | Where-Object { $_.Status -eq "FAIL" } | ForEach-Object {
        Out-Msg "    FAIL: $($_.Platform) / $($_.Label)" "Red"
    }
    Out-Msg "" "White"
    exit 1
} else {
    Out-Msg "  RESULT: ALL PASSED - safe to push" "Green"
    Out-Msg "" "White"
    exit 0
}
