<#
.SYNOPSIS
  Build, sign and package ProseCraft-rs for Windows.

.DESCRIPTION
  Produces, in $env:DIST (default: dist/release):
    prosecraft-rs-<version>-windows-<arch>.msi            per-user installer with a setup wizard (WiX v5)
    prosecraft-rs-<version>-windows-<arch>-portable.zip   prosecraft-rs.exe, prosecraft-mcp.exe and the notices, to run from anywhere

  prosecraft-rs.exe is a window program (PE subsystem 2: no console opens with it);
  prosecraft-mcp.exe, the MCP server an MCP client starts over stdio, is a console program
  (subsystem 3). Both link the C runtime statically (.cargo/config.toml), so neither needs the
  Visual C++ redistributable. The fonts come from a craft-fonts checkout (CRAFT_FONTS_DIR; a
  release build without them fails unless CRAFT_FONTS_REQUIRED=0). Signing goes through sign.ps1,
  which leaves the files unsigned with a warning when no signing secrets are set.

  Needs: Rust (MSVC toolchain + the target), the Windows SDK (signtool.exe, only for signing),
  and WiX v5: dotnet tool install --global wix --version 5.0.2
             wix extension add -g WixToolset.UI.wixext/5.0.2 WixToolset.Util.wixext/5.0.2

  Adapted from CraftCloud's packaging/windows/package.ps1, itself adapted from craft-launcher's
  (MIT OR Apache-2.0; see NOTICE), with WiX's own wizard art.

.EXAMPLE
  pwsh packaging/windows/package.ps1 -Arch x64
  pwsh packaging/windows/package.ps1 -Arch arm64 -SkipBuild
#>
param(
  [ValidateSet('x64', 'arm64')] [string] $Arch = 'x64',
  [switch] $SkipBuild
)
$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path

$Pkg = 'prosecraft-rs'    # cargo package and binary: the app
$McpPkg = 'prosecraft-mcp'  # cargo package and binary: the MCP server
$AppName = 'ProseCraft-rs'

function Invoke-Native([string] $What, [scriptblock] $Block) {
  Write-Output "==> $What"
  & $Block
  if ($LASTEXITCODE -ne 0) { throw "$What failed with exit code $LASTEXITCODE" }
}

# The version lives in one place: [workspace.package] version in the root Cargo.toml.
$Version = $env:PROSECRAFT_VERSION
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

$Target = switch ($Arch) { 'x64' { 'x86_64-pc-windows-msvc' } 'arm64' { 'aarch64-pc-windows-msvc' } }
$Dist = if ($env:DIST) { $env:DIST } else { Join-Path $Root 'dist\release' }
$TargetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $Root 'target' }
New-Item -ItemType Directory -Force -Path $Dist | Out-Null
# A release has the interface and manuscript fonts (crates/ui-egui/build.rs).
if (-not $env:CRAFT_FONTS_REQUIRED) { $env:CRAFT_FONTS_REQUIRED = '1' }

Write-Output "$AppName $Version for Windows $Arch ($Target)"

if (-not $SkipBuild) {
  # .cargo/config.toml links the C runtime statically for both MSVC targets.
  Invoke-Native "cargo build ($Target)" { cargo build --release --locked -p $Pkg -p $McpPkg --target $Target }
}

$Bin = Join-Path $TargetDir "$Target\release"
$Exe = "$Pkg.exe"
$McpExe = "$McpPkg.exe"

# Check the PE header before packaging. Machine (COFF header) must match -Arch, so an x64 build
# can never ship labelled arm64. Subsystem (optional header) must be 2 = Windows GUI for the app,
# so no console window opens with it, and 3 = Windows console for the MCP server, whose client
# talks to it over stdin and stdout.
function Get-PeHeader([string] $Path) {
  $bytes = [System.IO.File]::ReadAllBytes($Path)
  $pe = [BitConverter]::ToInt32($bytes, 0x3C)
  return @{ Machine = [BitConverter]::ToUInt16($bytes, $pe + 4); Subsystem = [BitConverter]::ToUInt16($bytes, $pe + 0x5C) }
}
$Machine = switch ($Arch) { 'x64' { 0x8664 } 'arm64' { 0xAA64 } }
foreach ($check in @(@{ File = $Exe; Subsystem = 2 }, @{ File = $McpExe; Subsystem = 3 })) {
  $h = Get-PeHeader (Join-Path $Bin $check.File)
  if ($h.Machine -ne $Machine) { throw "$($check.File) is for machine 0x$('{0:X}' -f $h.Machine), expected 0x$('{0:X}' -f $Machine) ($Arch)" }
  if ($h.Subsystem -ne $check.Subsystem) { throw "$($check.File) has PE subsystem $($h.Subsystem), expected $($check.Subsystem)" }
  Write-Output "ok $($check.File): $Arch, PE subsystem $($h.Subsystem)"
}

$Stage = Join-Path $TargetDir "windows-package\$Arch"
Remove-Item -Recurse -Force $Stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $Stage | Out-Null
$Icon = Join-Path $PSScriptRoot "$Pkg.ico"
Copy-Item -LiteralPath (Join-Path $Root 'NOTICE') -Destination $Stage
Copy-Item (Join-Path $Bin $Exe) $Stage
Copy-Item (Join-Path $Bin $McpExe) $Stage
Copy-Item $Icon $Stage

& (Join-Path $PSScriptRoot 'sign.ps1') (Join-Path $Stage $Exe) (Join-Path $Stage $McpExe)

# Run only on a machine of the build's architecture (an x64 runner can't run arm64): without
# PROSECRAFT_OPEN_DIR the MCP server must say so and fail at once, not wait on stdin.
$hostIsArm = $env:PROCESSOR_ARCHITECTURE -eq 'ARM64'
if (($Arch -eq 'arm64') -eq $hostIsArm) {
  $env:PROSECRAFT_OPEN_DIR = $null
  $out = '' | & (Join-Path $Stage $McpExe) 2>&1 | Out-String
  if ($LASTEXITCODE -eq 0 -or $out -notmatch 'PROSECRAFT_OPEN_DIR') { throw "$McpExe without PROSECRAFT_OPEN_DIR: exit $LASTEXITCODE, said: $out" }
  Write-Output "ok ${McpExe} without a folder: $($out.Trim())"
}

# ---- MSI ---------------------------------------------------------------------------------------
$Msi = Join-Path $Dist "$Pkg-$Version-windows-$Arch.msi"
Invoke-Native 'wix build' {
  wix build (Join-Path $PSScriptRoot "$Pkg.wxs") -arch $Arch `
    -ext WixToolset.UI.wixext -ext WixToolset.Util.wixext `
    -culture en-US -loc (Join-Path $PSScriptRoot "$Pkg.en-us.wxl") `
    -d "Version=$MsiVersion" -d "DisplayVersion=$Version" -d "BinDir=$Stage" -d "IconPath=$Icon" `
    -o $Msi
}
Invoke-Native 'MSI shortcut icon validation (ICE50)' {
  wix msi validate $Msi -ice ICE50 -intermediateFolder (Join-Path $Stage 'msi-validation')
}
# wix writes its debug symbols (.wixpdb) next to the MSI; keep them out of the release files.
Remove-Item -Force -ErrorAction SilentlyContinue ([IO.Path]::ChangeExtension($Msi, '.wixpdb'))
& (Join-Path $PSScriptRoot 'sign.ps1') $Msi

# ---- portable zip ------------------------------------------------------------------------------
# The same two programs without an installer (not on PATH). The app keeps its library in
# %APPDATA% like the installed one.
$Portable = Join-Path $TargetDir "windows-package\$Pkg-$Version-windows-$Arch-portable"
Remove-Item -Recurse -Force $Portable -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $Portable | Out-Null
Copy-Item (Join-Path $Stage $Exe) $Portable
Copy-Item (Join-Path $Stage $McpExe) $Portable
foreach ($f in 'LICENSE-MIT', 'LICENSE-APACHE', 'NOTICE') {
  $p = Join-Path $Root $f
  if (Test-Path $p) { Copy-Item $p $Portable }
}
Copy-Item (Join-Path $Root 'packaging\README-desktop.md') (Join-Path $Portable 'README.md')
$Zip = Join-Path $Dist "$Pkg-$Version-windows-$Arch-portable.zip"
Remove-Item -Force $Zip -ErrorAction SilentlyContinue
Compress-Archive -Path $Portable -DestinationPath $Zip

Get-Item $Msi, $Zip | Format-Table Name, Length
