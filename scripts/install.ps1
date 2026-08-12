<#
.SYNOPSIS
    isi-music — Windows install script
.DESCRIPTION
    Downloads the latest binary from GitHub Releases, adds it to PATH,
    and launches the setup wizard.
.EXAMPLE
    irm https://raw.githubusercontent.com/glrmrissi/isi_music/master/scripts/install.ps1 | iex
#>

$ErrorActionPreference = "Stop"

$Repo = "glrmrissi/isi_music"
$BinaryName = "isi-music.exe"
$InstallDir = "$env:LOCALAPPDATA\Programs\isi-music"

# Helpers
function Write-Ok($msg)    { Write-Host "  [OK]   $msg" -ForegroundColor Green }
function Write-Warn($msg)  { Write-Host "  [WARN] $msg" -ForegroundColor Yellow }
function Write-Fail($msg)  { Write-Host "  [FAIL] $msg" -ForegroundColor Red }
function Write-Info($msg)  { Write-Host "  [..]   $msg" -ForegroundColor Cyan }
function Write-Step($msg)  { Write-Host "  $msg" -ForegroundColor White }

# Banner
Write-Host ""
Write-Host "  isi-music - Windows Installer" -ForegroundColor Green
Write-Host "  $('-' * 50)"
Write-Host ""

# Step 1: Download binary
Write-Step "Step 1/2: Download isi-music"
Write-Host ""

$DownloadUrl = "https://github.com/$Repo/releases/latest/download/isi-music-windows-x86_64.exe"
$TargetPath = Join-Path $InstallDir $BinaryName

# Check if already installed
$ExistingPath = Get-Command isi-music -ErrorAction SilentlyContinue
if ($ExistingPath -and $args -notcontains "--force") {
    Write-Warn "isi-music is already installed at: $($ExistingPath.Source)"
    $Reinstall = Read-Host "  Reinstall? (y/N)"
    if ($Reinstall -notmatch '^[Yy]') {
        Write-Ok "Keeping existing installation"
        Write-Host ""
        $SkipDownload = $true
    }
}

if (-not $SkipDownload) {
    Write-Info "Creating install directory: $InstallDir"
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null

    Write-Info "Downloading latest release from GitHub..."
    try {
        Invoke-WebRequest -Uri $DownloadUrl -OutFile $TargetPath -UseBasicParsing
        Write-Ok "isi-music installed to $TargetPath"
    } catch {
        Write-Fail "Could not download binary. Check your internet connection or visit:"
        Write-Fail "  https://github.com/$Repo/releases"
        exit 1
    }

    # Add to user PATH if not already there
    $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($UserPath -notlike "*$InstallDir*") {
        Write-Info "Adding $InstallDir to user PATH..."
        [Environment]::SetEnvironmentVariable("Path", "$UserPath;$InstallDir", "User")
        $env:Path += ";$InstallDir"
        Write-Ok "Added to PATH (restart your terminal for changes to take effect)"
    } else {
        Write-Ok "Install directory already on PATH"
    }
}
Write-Host ""

$StartMenuDir = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs"
$ShortcutPath = Join-Path $StartMenuDir "isi-music.lnk"
$WindowsTerminal = Get-Command wt.exe -ErrorAction SilentlyContinue

if ($WindowsTerminal) {
    try {
        New-Item -ItemType Directory -Force -Path $StartMenuDir | Out-Null
        $escapedTargetPath = $TargetPath.Replace("'", "''")
        $shortcut = (New-Object -ComObject WScript.Shell).CreateShortcut($ShortcutPath)
        $shortcut.TargetPath = $WindowsTerminal.Source
        $shortcut.Arguments = "new-tab --title `"isi-music`" -- powershell.exe -NoLogo -NoExit -Command `"& '$escapedTargetPath'`"
        $shortcut.WorkingDirectory = $InstallDir
        $shortcut.IconLocation = "$TargetPath,0"
        $shortcut.Description = "isi-music in Windows Terminal"
        $shortcut.Save()
        Write-Ok "Start Menu shortcut created (Windows Terminal + PowerShell)"
    } catch {
        Write-Warn "Could not create Windows Terminal shortcut: $($_.Exception.Message)"
    }
} else {
    Write-Warn "Windows Terminal not found; isi-music will use the current console"
}
Write-Host ""

# Step 2: Setup wizard
Write-Step "Step 2/2: Setup wizard"
Write-Host ""

if (Get-Command isi-music -ErrorAction SilentlyContinue) {
    Write-Info "Launching setup wizard..."
    Write-Host ""
    try {
        & isi-music setup
    } catch {
        Write-Warn "Setup wizard exited with an error. You can re-run it later with: isi-music setup"
    }
} else {
    Write-Warn "isi-music not found on PATH. Skipping setup wizard."
    Write-Warn "Restart your terminal, then run: isi-music setup"
}
Write-Host ""

# Summary
Write-Host "  $('-' * 50)"
Write-Host "  Installation complete!" -ForegroundColor Green
Write-Host ""
Write-Host "  Next steps:"
Write-Host "    1. Restart your terminal (for PATH changes)"
Write-Host "    2. Run isi-music to start playing"
Write-Host "    3. Run isi-music doctor if something isn't working"
Write-Host ""
