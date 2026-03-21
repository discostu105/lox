<#
.SYNOPSIS
    Install lox (Loxone Miniserver CLI) on Windows.
.DESCRIPTION
    Downloads the latest lox release from GitHub and installs it to
    %LOCALAPPDATA%\lox. Adds the directory to the user PATH if needed.
.EXAMPLE
    irm https://raw.githubusercontent.com/discostu105/lox/main/install.ps1 | iex
#>

$ErrorActionPreference = 'Stop'

# Determine architecture
$arch = if ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture -eq 'Arm64') {
    'aarch64'
} else {
    'x86_64'
}

$repoOwner = 'discostu105'
$repoName = 'lox'
$artifactName = "lox-windows-$arch"

# Get latest release tag
Write-Host "Fetching latest release..." -ForegroundColor Cyan
$release = Invoke-RestMethod "https://api.github.com/repos/$repoOwner/$repoName/releases/latest"
$tag = $release.tag_name
Write-Host "Latest release: $tag" -ForegroundColor Green

# Find download URL
$asset = $release.assets | Where-Object { $_.name -eq "$artifactName.exe" }
if (-not $asset) {
    Write-Error "Could not find $artifactName.exe in release $tag. Available assets: $($release.assets.name -join ', ')"
    exit 1
}

$downloadUrl = $asset.browser_download_url

# Install directory
$installDir = Join-Path $env:LOCALAPPDATA 'lox'
if (-not (Test-Path $installDir)) {
    New-Item -ItemType Directory -Path $installDir -Force | Out-Null
}

$exePath = Join-Path $installDir 'lox.exe'

# Download
Write-Host "Downloading $artifactName.exe..." -ForegroundColor Cyan
Invoke-WebRequest -Uri $downloadUrl -OutFile $exePath -UseBasicParsing
Write-Host "Installed to $exePath" -ForegroundColor Green

# Add to PATH if not already present
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($userPath -notlike "*$installDir*") {
    [Environment]::SetEnvironmentVariable('Path', "$userPath;$installDir", 'User')
    $env:Path = "$env:Path;$installDir"
    Write-Host "Added $installDir to user PATH." -ForegroundColor Green
    Write-Host "Restart your terminal for PATH changes to take effect." -ForegroundColor Yellow
} else {
    Write-Host "$installDir is already in PATH." -ForegroundColor Green
}

# Verify
Write-Host ""
Write-Host "Installation complete! Run 'lox --help' to get started." -ForegroundColor Green
Write-Host ""
Write-Host "Quick setup:" -ForegroundColor Cyan
Write-Host "  lox setup set --host https://YOUR_MINISERVER_IP --user USER --pass PASS" -ForegroundColor White
