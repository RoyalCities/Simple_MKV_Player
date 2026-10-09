$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent $PSScriptRoot
$VendorFFmpeg = Join-Path $RepoRoot "vendor\ffmpeg"
$VendorMpv = Join-Path $RepoRoot "vendor\mpv"

New-Item -ItemType Directory -Path $VendorFFmpeg -Force | Out-Null
New-Item -ItemType Directory -Path $VendorMpv -Force | Out-Null

$FFmpeg = Get-Command "ffmpeg.exe" -ErrorAction SilentlyContinue
$FFprobe = Get-Command "ffprobe.exe" -ErrorAction SilentlyContinue

if (-not $FFmpeg) {
    throw "ffmpeg.exe was not found on PATH."
}

if (-not $FFprobe) {
    throw "ffprobe.exe was not found on PATH."
}

Copy-Item $FFmpeg.Source (Join-Path $VendorFFmpeg "ffmpeg.exe") -Force
Copy-Item $FFprobe.Source (Join-Path $VendorFFmpeg "ffprobe.exe") -Force

$MpvDll = Join-Path $VendorMpv "libmpv-2.dll"

if (-not (Test-Path $MpvDll)) {
    throw "libmpv-2.dll is missing from vendor\mpv. Keep the known working libmpv DLL there before building a release."
}

Write-Host ""
Write-Host "Runtime staged:"
Write-Host "  ffmpeg.exe <- $($FFmpeg.Source)"
Write-Host "  ffprobe.exe <- $($FFprobe.Source)"
Write-Host "  libmpv-2.dll <- $MpvDll"
Write-Host ""
Write-Host "The release builder will package these exact files."
