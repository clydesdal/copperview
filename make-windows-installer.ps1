# Builds dist\copperview.exe and the Windows installer,
# dist\copperview-setup-<version>.exe, from assets\windows\copperview.iss.
# Needs Rust and Inno Setup 6 (winget install JRSoftware.InnoSetup):
#   powershell -ExecutionPolicy Bypass -File make-windows-installer.ps1
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.*)"' | Select-Object -First 1).Matches[0].Groups[1].Value
cargo build --release --locked
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

New-Item -ItemType Directory -Force dist | Out-Null
Copy-Item -Force target\release\copperview.exe dist\copperview.exe

# Inno Setup installs for everyone or for one person, so look in both places.
$iscc = (Get-Command iscc -ErrorAction SilentlyContinue).Source
if (-not $iscc) {
    $iscc = @("${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
              "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
              "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe") | Where-Object { Test-Path $_ } | Select-Object -First 1
}
if (-not $iscc) { throw "Inno Setup 6 isn't installed: winget install JRSoftware.InnoSetup" }

# The script's own paths are relative to assets\windows; only the version is passed in.
& $iscc /Q "/DVersion=$version" assets\windows\copperview.iss
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
"dist\copperview-setup-$version.exe"
