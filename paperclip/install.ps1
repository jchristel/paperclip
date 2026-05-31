# install.ps1
# Per-user installer for paperclip -- no admin rights required

$AppName   = "paperclip"
$ExeName   = "paperclip.exe"
$SourceExe = Join-Path $PSScriptRoot "target\release\$ExeName"

# --- Verify the release binary exists ------------------------------------

if (-not (Test-Path $SourceExe)) {
    Write-Host "ERROR: Release binary not found at:" -ForegroundColor Red
    Write-Host "  $SourceExe" -ForegroundColor Red
    Write-Host ""
    Write-Host "Please run 'cargo build --release' first." -ForegroundColor Yellow
    exit 1
}

# --- Ask the user for the install folder ---------------------------------

$DefaultInstallDir = Join-Path $env:LOCALAPPDATA $AppName
Write-Host ""
Write-Host "Paperclip Installer" -ForegroundColor Cyan
Write-Host "-------------------"
Write-Host "Default install folder: $DefaultInstallDir"
Write-Host ""
$UserInput = Read-Host "Press Enter to accept, or type a different path"

# If the user just pressed Enter, use the default
if ($UserInput.Trim() -eq "") {
    $InstallDir = $DefaultInstallDir
} else {
    $InstallDir = $UserInput.Trim()
}

# --- Create the install folder if needed ---------------------------------

if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir | Out-Null
    Write-Host "Created folder: $InstallDir" -ForegroundColor Green
}

# --- Copy the binary -----------------------------------------------------

Copy-Item -Path $SourceExe -Destination $InstallDir -Force
Write-Host "Copied $ExeName to $InstallDir" -ForegroundColor Green

# --- Add to user-level PATH (HKCU -- no admin needed) --------------------

# HKCU = current user only, no elevation required
# HKLM = all users, requires admin -- we deliberately avoid that
$RegPath     = "HKCU:\Environment"
$CurrentPath = (Get-ItemProperty -Path $RegPath -Name Path -ErrorAction SilentlyContinue).Path

if ($CurrentPath -like "*$InstallDir*") {
    Write-Host "PATH already contains $InstallDir -- skipping." -ForegroundColor Yellow
} else {
    if ($CurrentPath) {
        $NewPath = "$CurrentPath;$InstallDir"
    } else {
        $NewPath = $InstallDir
    }
    Set-ItemProperty -Path $RegPath -Name Path -Value $NewPath
    Write-Host "Added to user PATH: $InstallDir" -ForegroundColor Green
}

# Notify the system that the environment has changed
# This makes the PATH update visible to cmd without needing a reboot
$signature = '[DllImport("user32.dll")] public static extern int SendMessageTimeout(IntPtr hWnd, uint Msg, IntPtr wParam, string lParam, uint fuFlags, uint uTimeout, IntPtr lpdwResult);'
$type = Add-Type -MemberDefinition $signature -Name WinEnv -Namespace Win32 -PassThru
$type::SendMessageTimeout([IntPtr]0xffff, 0x001A, [IntPtr]::Zero, "Environment", 2, 5000, [IntPtr]::Zero)
# --- Done ----------------------------------------------------------------

Write-Host ""
Write-Host "Installation complete." -ForegroundColor Cyan
Write-Host "Open a new terminal window and run: paperclip --help"
Write-Host ""
