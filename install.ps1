$ErrorActionPreference = 'Stop'

# GitHub release URL
$releaseUrl = "https://github.com/tellfesen/pgm/releases/latest/download/pgm-windows.exe"

# Download the binary
Write-Host "Downloading pgm..."
Invoke-WebRequest -Uri $releaseUrl -OutFile "pgm.exe"

# Add to PATH
$installDir = "$env:LOCALAPPDATA\Programs\pgm"
New-Item -ItemType Directory -Force -Path $installDir | Out-Null
Move-Item -Path "pgm.exe" -Destination "$installDir\pgm.exe" -Force

if ($env:PATH -notlike "*$installDir*") {
    [Environment]::SetEnvironmentVariable("PATH", "$env:PATH;$installDir", "User")
    $env:PATH = "$env:PATH;$installDir"
}

Write-Host "pgm has been installed successfully!"
Write-Host "Please restart your terminal or run 'refreshenv' to use pgm."