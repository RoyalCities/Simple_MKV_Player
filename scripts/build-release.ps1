param(
    [switch]$SkipInstaller
)

$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent $PSScriptRoot
$DistRoot = Join-Path $RepoRoot "dist"
$StageRoot = Join-Path $DistRoot "Simple MKV Player"

Set-Location $RepoRoot

$CargoToml = Get-Content (Join-Path $RepoRoot "Cargo.toml") -Raw
$VersionMatch = [regex]::Match($CargoToml, '(?m)^version\s*=\s*"([^"]+)"')

if (-not $VersionMatch.Success) {
    throw "Could not determine the package version from Cargo.toml."
}

$Version = $VersionMatch.Groups[1].Value

$MpvDll = Join-Path $RepoRoot "vendor\mpv\libmpv-2.dll"
$FFmpegExe = Join-Path $RepoRoot "vendor\ffmpeg\ffmpeg.exe"
$FFprobeExe = Join-Path $RepoRoot "vendor\ffmpeg\ffprobe.exe"

foreach ($RequiredFile in @($MpvDll, $FFmpegExe, $FFprobeExe)) {
    if (-not (Test-Path $RequiredFile)) {
        throw "Required runtime file not found: $RequiredFile`nRun .\scripts\stage-local-runtime.ps1 or place the file in vendor manually."
    }
}

Write-Host "Building Simple MKV Player v$Version..."
cargo build --release

if ($LASTEXITCODE -ne 0) {
    throw "cargo build --release failed."
}

if (Test-Path $StageRoot) {
    Remove-Item $StageRoot -Recurse -Force
}

New-Item -ItemType Directory -Path $StageRoot -Force | Out-Null

$BuiltExe = Join-Path $RepoRoot "target\release\simple_mkv_player.exe"
$AppIcon = Join-Path $RepoRoot "src\assets\smkv_logo.ico"
$LicensePath = Join-Path $RepoRoot "LICENSE"
$NoticesPath = Join-Path $RepoRoot "THIRD_PARTY_NOTICES.md"

foreach ($RequiredFile in @(
    $BuiltExe,
    $AppIcon,
    $LicensePath,
    $NoticesPath
)) {
    if (-not (Test-Path $RequiredFile)) {
        throw "Required release file not found: $RequiredFile"
    }
}

Copy-Item $BuiltExe (Join-Path $StageRoot "Simple MKV Player.exe") -Force
Copy-Item $MpvDll $StageRoot -Force
Copy-Item $FFmpegExe $StageRoot -Force
Copy-Item $FFprobeExe $StageRoot -Force
Copy-Item $AppIcon $StageRoot -Force
Copy-Item (Join-Path $RepoRoot "README.md") $StageRoot -Force
Copy-Item $LicensePath $StageRoot -Force
Copy-Item $NoticesPath $StageRoot -Force

$StageLicenses = Join-Path $StageRoot "licenses"
New-Item -ItemType Directory -Path $StageLicenses -Force | Out-Null

Get-ChildItem (Join-Path $RepoRoot "third_party\licenses") -File -ErrorAction SilentlyContinue |
    ForEach-Object {
        Copy-Item $_.FullName $StageLicenses -Force
    }

$ManifestPath = Join-Path $StageRoot "DEPENDENCY_MANIFEST.txt"
$ManifestLines = @(
    "Simple MKV Player v$Version release dependency manifest",
    "Generated UTC: $([DateTime]::UtcNow.ToString('o'))",
    "",
    "Runtime files packaged with this build:",
    ""
)

Get-ChildItem $StageRoot -File |
    Where-Object { $_.Extension -in @(".exe", ".dll") } |
    Sort-Object Name |
    ForEach-Object {
        $Hash = (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        $ManifestLines += "$Hash  $($_.Name)"
    }

$ManifestLines | Set-Content -Path $ManifestPath -Encoding UTF8

$PortableZip = Join-Path $DistRoot "Simple_MKV_Player_v${Version}_Portable.zip"

if (Test-Path $PortableZip) {
    Remove-Item $PortableZip -Force
}

Compress-Archive -Path $StageRoot -DestinationPath $PortableZip -CompressionLevel Optimal
Write-Host "Portable package: $PortableZip"

if ($SkipInstaller) {
    exit 0
}

$IsccCandidates = @(
    (Get-Command "ISCC.exe" -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source -ErrorAction SilentlyContinue),
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -Unique

$Iscc = $IsccCandidates | Select-Object -First 1

if (-not $Iscc) {
    Write-Warning "Inno Setup 6 was not found. Portable ZIP was created; installer was skipped."
    Write-Warning "Install Inno Setup 6, then run this script again to also create Setup.exe."
    exit 0
}

$InstallerScript = Join-Path $RepoRoot "packaging\SimpleMKVPlayer.iss"

& $Iscc `
    "/DSourceDir=$StageRoot" `
    "/DAppVersion=$Version" `
    "/DOutputDir=$DistRoot" `
    $InstallerScript

if ($LASTEXITCODE -ne 0) {
    throw "Inno Setup failed."
}

Write-Host "Installer package created in: $DistRoot"
