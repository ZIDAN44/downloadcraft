<#
.SYNOPSIS
  Build, sign and package DownloadCraft for Windows.

.DESCRIPTION
  Produces, in $env:DIST (default: dist/release):
    downloadcraft-<version>-windows-<arch>.msi            per-user installer, no administrator needed (WiX v5)
    downloadcraft-<version>-windows-<arch>-portable.zip   downloadcraft.exe + downloadcraft-cli.exe + portable.txt

  The binaries link the C runtime statically (+crt-static), so neither the MSI nor the portable
  zip needs the Visual C++ redistributable. Signing is delegated to sign.ps1 (skipped with a
  warning when no signing secrets are set).

  Needs: Rust (MSVC toolchain + the target), the Windows SDK (rc.exe, signtool.exe),
  and WiX v5 with its UI and Util extensions:
    dotnet tool install --global wix --version 5.0.2
    wix extension add -g WixToolset.UI.wixext/5.0.2 WixToolset.Util.wixext/5.0.2

.EXAMPLE
  pwsh packaging/windows/package.ps1 -Arch x64
  pwsh packaging/windows/package.ps1 -Arch x86 -SkipBuild
  pwsh packaging/windows/package.ps1 -Arch arm64     # cross-compiled; needs the MSVC ARM64 build tools
#>
param(
  [ValidateSet('x64', 'x86', 'arm64')] [string] $Arch = 'x64',
  [switch] $SkipBuild
)
$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path

function Invoke-Native([string] $What, [scriptblock] $Block) {
  Write-Output "==> $What"
  & $Block
  if ($LASTEXITCODE -ne 0) { throw "$What failed with exit code $LASTEXITCODE" }
}

# The version lives in one place: [workspace.package] version in the root Cargo.toml.
$Version = $env:DOWNLOADCRAFT_VERSION
if (-not $Version) {
  $inPkg = $false
  foreach ($line in Get-Content (Join-Path $Root 'Cargo.toml')) {
    if ($line -match '^\s*\[') { $inPkg = ($line.Trim() -eq '[workspace.package]'); continue }
    if ($inPkg -and $line -match '^\s*version\s*=\s*"([^"]+)"') { $Version = $Matches[1]; break }
  }
}
if (-not $Version) { throw 'could not read [workspace.package] version from Cargo.toml' }
# MSI ProductVersion is numeric (major.minor.build); pre-release tags are dropped there.
$MsiVersion = ($Version -split '-')[0]

$Target = switch ($Arch) { 'x64' { 'x86_64-pc-windows-msvc' } 'x86' { 'i686-pc-windows-msvc' } 'arm64' { 'aarch64-pc-windows-msvc' } }
$Dist = if ($env:DIST) { $env:DIST } else { Join-Path $Root 'dist\release' }
$TargetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $Root 'target' }
New-Item -ItemType Directory -Force -Path $Dist | Out-Null

# Shown by `downloadcraft --version`. The shared release workflow sets BUILD_SHA and BUILD_DATE.
if (-not $env:DOWNLOADCRAFT_BUILD_SHA) { $env:DOWNLOADCRAFT_BUILD_SHA = if ($env:BUILD_SHA) { $env:BUILD_SHA } else { (git -C $Root rev-parse HEAD 2>$null) } }
if (-not $env:DOWNLOADCRAFT_BUILD_DATE) { $env:DOWNLOADCRAFT_BUILD_DATE = if ($env:BUILD_DATE) { $env:BUILD_DATE } else { (Get-Date).ToUniversalTime().ToString('yyyy-MM-dd') } }

Write-Output "DownloadCraft $Version for Windows $Arch ($Target)"

if (-not $SkipBuild) {
  # Static CRT: no VC++ redistributable needed. Scoped to the target so host build scripts and
  # proc-macros are unaffected.
  $flagVar = 'CARGO_TARGET_' + ($Target.ToUpper() -replace '-', '_') + '_RUSTFLAGS'
  [Environment]::SetEnvironmentVariable($flagVar, '-C target-feature=+crt-static')
  # Fail the build (rather than warn) if the icon/VERSIONINFO can't be embedded.
  $env:DOWNLOADCRAFT_REQUIRE_WINRES = '1'
  Invoke-Native "cargo build ($Target)" { cargo build --release --locked -p downloadcraft -p downloadcraft-cli --target $Target }
}

$Bin = Join-Path $TargetDir "$Target\release"

# Check both binaries' PE headers before packaging. Machine (COFF header) must match -Arch, so an
# x64 build can never ship labelled arm64. Subsystem (optional header): 2 = Windows GUI, 3 = console.
# The app must be GUI (no console window opens with it); the CLI must stay console so its output
# reaches the terminal.
function Get-PeHeader([string] $Path) {
  $bytes = [System.IO.File]::ReadAllBytes($Path)
  $pe = [BitConverter]::ToInt32($bytes, 0x3C)
  return @{ Machine = [BitConverter]::ToUInt16($bytes, $pe + 4); Subsystem = [BitConverter]::ToUInt16($bytes, $pe + 0x5C) }
}
$Machine = switch ($Arch) { 'x64' { 0x8664 } 'x86' { 0x14C } 'arm64' { 0xAA64 } }
foreach ($check in @(@('downloadcraft.exe', 2), @('downloadcraft-cli.exe', 3))) {
  $h = Get-PeHeader (Join-Path $Bin $check[0])
  if ($h.Machine -ne $Machine) { throw "$($check[0]) is for machine 0x$('{0:X}' -f $h.Machine), expected 0x$('{0:X}' -f $Machine) ($Arch)" }
  if ($h.Subsystem -ne $check[1]) { throw "$($check[0]) has PE subsystem $($h.Subsystem), expected $($check[1])" }
  Write-Output "ok $($check[0]): $Arch, PE subsystem $($h.Subsystem)"
}
$Stage = Join-Path $TargetDir "windows-package\$Arch"
Remove-Item -Recurse -Force $Stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $Stage | Out-Null
Copy-Item (Join-Path $Bin 'downloadcraft.exe'), (Join-Path $Bin 'downloadcraft-cli.exe') $Stage

& (Join-Path $PSScriptRoot 'sign.ps1') (Join-Path $Stage 'downloadcraft.exe') (Join-Path $Stage 'downloadcraft-cli.exe')

# ---- MSI ---------------------------------------------------------------------------------------
# The setup wizard's art, drawn here (System.Drawing) from DownloadCraft's icon: the banner over the inner pages (493x58) and the Welcome and Finish pages' picture
# (493x312, the text sits on the right 329 px of it).
function New-InstallerArt([string] $BannerOut, [string] $DialogOut) {
  Add-Type -AssemblyName System.Drawing
  $top = [System.Drawing.ColorTranslator]::FromHtml('#3a8aee')
  $bottom = [System.Drawing.ColorTranslator]::FromHtml('#1758b4')
  $icon = [System.Drawing.Image]::FromFile((Join-Path $Root 'assets\app-icon\hicolor\256x256\apps\ai.storyteller.downloadcraft.png'))
  try {
    foreach ($spec in @(@($BannerOut, 493, 58), @($DialogOut, 493, 312))) {
      $bmp = New-Object System.Drawing.Bitmap $spec[1], $spec[2], ([System.Drawing.Imaging.PixelFormat]::Format24bppRgb)
      $g = [System.Drawing.Graphics]::FromImage($bmp)
      try {
        $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
        $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
        $g.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::AntiAliasGridFit
        $g.Clear([System.Drawing.Color]::White)
        if ($spec[2] -eq 58) {
          $g.DrawImage($icon, 433, 9, 40, 40)
        } else {
          $panel = New-Object System.Drawing.Rectangle 0, 0, 164, 312
          $brush = New-Object System.Drawing.Drawing2D.LinearGradientBrush $panel, $top, $bottom, 90
          $g.FillRectangle($brush, $panel)
          $brush.Dispose()
          $g.DrawImage($icon, 42, 70, 80, 80)
          $font = New-Object System.Drawing.Font 'Segoe UI', 13, ([System.Drawing.FontStyle]::Bold), ([System.Drawing.GraphicsUnit]::Pixel)
          $format = New-Object System.Drawing.StringFormat
          $format.Alignment = [System.Drawing.StringAlignment]::Center
          $g.DrawString('DownloadCraft', $font, [System.Drawing.Brushes]::White, (New-Object System.Drawing.RectangleF 0, 166, 164, 24), $format)
          $font.Dispose()
        }
        $bmp.Save($spec[0], [System.Drawing.Imaging.ImageFormat]::Bmp)
      } finally {
        $g.Dispose()
        $bmp.Dispose()
      }
    }
  } finally {
    $icon.Dispose()
  }
}
$Banner = Join-Path $Stage 'installer-banner.bmp'
$Dialog = Join-Path $Stage 'installer-dialog.bmp'
New-InstallerArt $Banner $Dialog

$Msi = Join-Path $Dist "downloadcraft-$Version-windows-$Arch.msi"
Invoke-Native 'wix build' {
  wix build (Join-Path $PSScriptRoot 'downloadcraft.wxs') -arch $Arch `
    -ext WixToolset.UI.wixext -ext WixToolset.Util.wixext `
    -culture en-US -loc (Join-Path $PSScriptRoot 'downloadcraft.en-us.wxl') `
    -d "Version=$MsiVersion" -d "DisplayVersion=$Version" -d "BinDir=$Stage" -d "IconPath=$(Join-Path $Root 'assets\app-icon\downloadcraft.ico')" `
    -d "BannerBmp=$Banner" -d "DialogBmp=$Dialog" `
    -o $Msi
}
# The wizard's art isn't shipped beside the exe.
Remove-Item -Force $Banner, $Dialog
# wix writes its debug symbols (.wixpdb) next to the MSI; keep them out of the release assets.
Remove-Item -Force -ErrorAction SilentlyContinue ([IO.Path]::ChangeExtension($Msi, '.wixpdb'))
& (Join-Path $PSScriptRoot 'sign.ps1') $Msi

# ---- portable zip ------------------------------------------------------------------------------
$Portable = Join-Path $TargetDir "windows-package\downloadcraft-$Version-windows-$Arch-portable"
Remove-Item -Recurse -Force $Portable -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $Portable | Out-Null
Copy-Item (Join-Path $Stage '*.exe') $Portable
foreach ($f in 'README.md', 'LICENSE', 'LICENSE-MIT', 'LICENSE-APACHE', 'ATTRIBUTION.md') {
  $p = Join-Path $Root $f
  if (Test-Path $p) { Copy-Item $p $Portable }
}
# portable.txt beside downloadcraft.exe switches on portable mode: the list and settings go to
# DownloadCraftData\ next to the exe (apps/downloadcraft/src/store.rs).
Copy-Item (Join-Path $PSScriptRoot 'portable.txt') $Portable
$Zip = Join-Path $Dist "downloadcraft-$Version-windows-$Arch-portable.zip"
Remove-Item -Force $Zip -ErrorAction SilentlyContinue
Compress-Archive -Path $Portable -DestinationPath $Zip

# Smoke-test the CLI when this machine can run it. An ARM64 build made on an x64 runner can't run
# here.
$HostArch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()
if ($Arch -ne 'arm64' -or $HostArch -eq 'arm64') {
  Invoke-Native 'downloadcraft-cli --version' { & (Join-Path $Stage 'downloadcraft-cli.exe') --version }
} else {
  Write-Output "skipping downloadcraft-cli --version: an $Arch build doesn't run on this $HostArch machine"
}
Get-Item $Msi, $Zip | Format-Table Name, Length
