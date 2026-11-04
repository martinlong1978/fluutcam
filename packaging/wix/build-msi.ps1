# Builds the Windows installer from the (signed) executables.
#
#   pwsh packaging/wix/build-msi.ps1 -Version 0.1.0 -BinDir bin -IconFile icons\windows\fluutcam.ico -Out dist
#
# Writes <Out>\fluutcam-<Version>-windows-x86_64.msi. Used by release.yml and
# packaging.yml. WiX is pinned here; it is installed as a .NET tool when `wix`
# is not on the PATH or has a different version.
param(
  [Parameter(Mandatory)] [string] $Version,
  [Parameter(Mandatory)] [string] $BinDir,
  [Parameter(Mandatory)] [string] $IconFile,
  [Parameter(Mandatory)] [string] $Out,
  [string] $RootDir = (Join-Path $PSScriptRoot '..\..')
)
$ErrorActionPreference = 'Stop'
$WixVersion = '5.0.2'

# MSI versions are numeric: major.minor.build. The pre-release suffix of
# "1.2.3-rc.1" is dropped; packages of the same number replace each other.
if ($Version -notmatch '^(\d+)\.(\d+)\.(\d+)') { throw "version '$Version' is not X.Y.Z" }
$msiVersion = "$($Matches[1]).$($Matches[2]).$($Matches[3])"
if ([int]$Matches[1] -gt 255 -or [int]$Matches[2] -gt 255 -or [int]$Matches[3] -gt 65535) {
  throw "version '$Version' does not fit an MSI product version"
}

$wix = Get-Command wix -ErrorAction SilentlyContinue
if (-not $wix -or -not ((& wix --version) -like "$WixVersion*")) {
  dotnet tool install --global wix --version $WixVersion
  if ($LASTEXITCODE -ne 0) { throw 'wix install failed' }
  $env:PATH = "$env:USERPROFILE\.dotnet\tools$([IO.Path]::PathSeparator)$env:PATH"
}

foreach ($f in "$BinDir\fluutcam.exe", "$BinDir\fluut.exe", $IconFile,
               "$RootDir\LICENSE-APACHE", "$RootDir\LICENSE-MIT") {
  if (-not (Test-Path $f)) { throw "missing input $f" }
}

New-Item -ItemType Directory -Force $Out | Out-Null
$msi = Join-Path $Out "fluutcam-$Version-windows-x86_64.msi"
wix build -arch x64 -o $msi `
  -d "Version=$msiVersion" -d "BinDir=$BinDir" -d "IconFile=$IconFile" -d "RootDir=$RootDir" `
  (Join-Path $PSScriptRoot 'fluutcam.wxs')
if ($LASTEXITCODE -ne 0) { throw 'wix build failed' }

# ICE validation. ICE91 (per-user files) and ICE61 (same-version upgrades) are
# expected; any other error fails the build.
$report = wix msi validate $msi 2>&1 | Out-String
$report
if ($report -match 'error WIX') { throw 'MSI validation failed' }
Write-Host "wrote $msi"
