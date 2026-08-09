<#
.SYNOPSIS
    isi-music — Windows install script
.DESCRIPTION
    Downloads the latest binary from GitHub Releases, adds it to PATH,
    installs a Nerd Font (if missing), and launches the setup wizard.
.EXAMPLE
    irm https://raw.githubusercontent.com/glrmrissi/isi_music/main/scripts/install.ps1 | iex
#>

$ErrorActionPreference = "Stop"

$Repo = "glrmrissi/isi_music"
$BinaryName = "isi-music.exe"
$InstallDir = "$env:LOCALAPPDATA\Programs\isi-music"
$NerdFontVersion = "3.3.0"
$NerdFontName = "FiraCode"
$NerdFontZip = "FiraCode.zip"

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
Write-Step "Step 1/3: Download isi-music"
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

# Step 2: Nerd Font
Write-Step "Step 2/3: Nerd Font"
Write-Host ""

# Check if a Nerd Font is already installed
$InstalledFonts = Get-ChildItem "C:\Windows\Fonts","$env:LOCALAPPDATA\Microsoft\Windows\Fonts" -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -like "*Nerd*" -or $_.Name -like "*NF*" }

if ($InstalledFonts) {
    Write-Ok "Nerd Font already installed"
    $SkipFont = $true
}

if (-not $SkipFont) {
    Write-Info "Downloading $NerdFontName Nerd Font v$NerdFontVersion..."
    $TempZip = Join-Path $env:TEMP $NerdFontZip
    $FontUrl = "https://github.com/ryanoasis/nerd-fonts/releases/download/v$NerdFontVersion/$NerdFontZip"

    try {
        Invoke-WebRequest -Uri $FontUrl -OutFile $TempZip -UseBasicParsing

        $ExtractDir = Join-Path $env:TEMP "nerd-font-extract"
        if (Test-Path $ExtractDir) { Remove-Item $ExtractDir -Recurse -Force }
        Expand-Archive -Path $TempZip -DestinationPath $ExtractDir -Force

        # Install .ttf files
        $UserFontDir = "$env:LOCALAPPDATA\Microsoft\Windows\Fonts"
        New-Item -ItemType Directory -Force -Path $UserFontDir | Out-Null

        $TtfFiles = Get-ChildItem $ExtractDir -Filter "*.ttf"
        $FontShell = New-Object -ComObject Shell.Application
        $FontsFolder = $FontShell.Namespace(0x14)  # Fonts folder

        foreach ($ttf in $TtfFiles) {
            $destPath = Join-Path $UserFontDir $ttf.Name
            Copy-Item $ttf.FullName $destPath -Force
            # Also register via shell (per-user install on Win10+)
            try {
                $FontsFolder.CopyHere($ttf.FullName, 0x10)
            } catch {
                # Shell install may fail in non-interactive; the file copy is enough
            }
        }

        Remove-Item $TempZip -Force
        Remove-Item $ExtractDir -Recurse -Force -ErrorAction SilentlyContinue

        Write-Ok "$NerdFontName Nerd Font installed"
        Write-Warn "Configure your terminal (Windows Terminal) to use '$NerdFontName Nerd Font'"
    } catch {
        Write-Fail "Could not download Nerd Font. Install manually from:"
        Write-Fail "  https://www.nerdfonts.com/font-downloads"
    }
}
Write-Host ""

# Step 3: Setup wizard
Write-Step "Step 3/3: Setup wizard"
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
Write-Host "    1. Configure your terminal to use a Nerd Font (if not done)"
Write-Host "    2. Restart your terminal (for PATH changes)"
Write-Host "    3. Run isi-music to start playing"
Write-Host "    4. Run isi-music doctor if something isn't working"
Write-Host ""
