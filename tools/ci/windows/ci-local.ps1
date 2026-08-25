#Requires -Version 5.1
# ci-local.ps1 - Emulates the GitHub Actions pipeline locally
# Replicates: ci.yml + format.yml + release.yml (test/build jobs)
# Prefer ci-all.ps1 (this is the legacy single-pipeline variant).
#
# Usage: .\scripts\ci\windows\ci-local.ps1
#        .\scripts\ci\windows\ci-local.ps1 -SkipBuild    (skip release build)
#        .\scripts\ci\windows\ci-local.ps1 -SkipTest     (skip tests)
#        .\scripts\ci\windows\ci-local.ps1 -Only deny    (run only cargo deny)

[CmdletBinding()]
param(
    [switch]$SkipBuild,
    [switch]$SkipTest,
    [switch]$SkipDeny,
    [switch]$SkipClippy,
    [switch]$SkipFmt,
    [string]$Only
)

$ErrorActionPreference = "Continue"
$root = Resolve-Path "$PSScriptRoot\..\..\.."
Set-Location $root

$pass = 0
$fail = 0
$skipped = 0
$totalStart = Get-Date

function Invoke-Step {
    param(
        [string]$Name,
        [scriptblock]$Action,
        [bool]$Skip = $false
    )

    if ($Skip) {
        Write-Host "  [SKIP] $Name" -ForegroundColor DarkGray
        $script:skipped++
        return $true
    }

    $start = Get-Date
    Write-Host "  [RUN]  $Name" -ForegroundColor Cyan -NoNewline

    $output = & $Action 2>&1
    $code = $LASTEXITCODE
    if ($null -eq $code) { $code = 0 }

    if ($code -ne 0) {
        $output | Select-Object -Last 5 | ForEach-Object { Write-Host "`n      $_" -ForegroundColor DarkRed }
        $elapsed = ((Get-Date) - $start).TotalSeconds
        Write-Host "`r  [FAIL] $Name ($('{0:N1}' -f $elapsed)s)" -ForegroundColor Red
        $script:fail++
        return $false
    }

    $elapsed = ((Get-Date) - $start).TotalSeconds
    Write-Host "`r  [PASS] $Name ($('{0:N1}' -f $elapsed)s)" -ForegroundColor Green
    $script:pass++
    return $true
}

# With -Only, run only that step
$onlyDeny   = ($Only -eq "deny")
$onlyFmt    = ($Only -eq "fmt")
$onlyClippy = ($Only -eq "clippy")
$onlyTest   = ($Only -eq "test")
$onlyBuild  = ($Only -eq "build")
$hasOnly    = -not [string]::IsNullOrEmpty($Only)

Write-Host ""
Write-Host "=== isi_music CI Local Pipeline ===" -ForegroundColor Yellow
Write-Host ""

# 1. cargo fmt --check (format.yml)
Invoke-Step "cargo fmt --check" {
    cargo fmt --check
} -Skip:($SkipFmt -or ($hasOnly -and -not $onlyFmt))

# 2. cargo deny (ci.yml + release.yml)
Invoke-Step "cargo deny --all-features --locked check" {
    cargo deny --all-features --locked check
} -Skip:($SkipDeny -or ($hasOnly -and -not $onlyDeny))

# 3. cargo clippy (pre-commit hook + AGENTS.md, not part of GitHub CI)
Invoke-Step "cargo clippy --all-targets --all-features --locked -- -D warnings" {
    cargo clippy --all-targets --all-features --locked -- -D warnings
} -Skip:($SkipClippy -or ($hasOnly -and -not $onlyClippy))

# 4. cargo test --locked (release.yml: default features)
Invoke-Step "cargo test --locked (default features)" {
    cargo test --locked
} -Skip:($SkipTest -or ($hasOnly -and -not $onlyTest))

# 5. cargo test --locked --no-default-features -F spotify,discord (release.yml: minimal)
Invoke-Step "cargo test --locked (minimal: spotify,discord)" {
    cargo test --locked --no-default-features -F spotify,discord
} -Skip:($SkipTest -or ($hasOnly -and -not $onlyTest))

# 6. cargo build --release --locked (ci.yml)
Invoke-Step "cargo build --release --locked (default features)" {
    cargo build --release --locked
} -Skip:($SkipBuild -or ($hasOnly -and -not $onlyBuild))

# 7. cargo build --release --locked -F mpris (release.yml: full binary)
Invoke-Step "cargo build --release --locked -F mpris (full)" {
    cargo build --release --locked -F mpris
} -Skip:($SkipBuild -or ($hasOnly -and -not $onlyBuild))

# 8. cargo build --release --locked --no-default-features -F spotify,discord (release.yml: minimal binary)
Invoke-Step "cargo build --release --locked (minimal: spotify,discord)" {
    cargo build --release --locked --no-default-features -F spotify,discord
} -Skip:($SkipBuild -or ($hasOnly -and -not $onlyBuild))

# Summary
$totalElapsed = ((Get-Date) - $totalStart).TotalSeconds
Write-Host ""
Write-Host "=== Results ===" -ForegroundColor Yellow
Write-Host "  PASS:    $pass" -ForegroundColor Green
Write-Host "  FAIL:    $fail" -ForegroundColor $(if ($fail -gt 0) { "Red" } else { "DarkGray" })
Write-Host "  SKIPPED: $skipped" -ForegroundColor DarkGray
Write-Host "  Time:    $('{0:N1}' -f $totalElapsed)s" -ForegroundColor DarkGray
Write-Host ""

if ($fail -gt 0) {
    Write-Host "CI FAILED: fix errors before pushing" -ForegroundColor Red
    exit 1
} else {
    Write-Host "CI PASSED: safe to push" -ForegroundColor Green
    exit 0
}
