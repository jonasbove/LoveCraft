<#
.SYNOPSIS
  Install-test the MSI as a person would, and the update from the previous release's MSI.

.DESCRIPTION
  1. With -Previous: the previous release's MSI is installed first (silently, per user), and its
     app answers --version.
  2. This MSI is installed over it. Windows Installer must replace the previous product (the same
     UpgradeCode) in its folder: one product left, named LoveCraft, in the folder the previous
     one was in. lovecraft.exe and, under the 0.1 names (LoveCraft was ProseCraft-rs until 0.2.0),
     prosecraft-rs.exe both answer --version with this version; lovecraft-mcp.exe and
     prosecraft-mcp.exe answer an MCP `initialize` and `tools/list`; the Start menu has LoveCraft
     and no ProseCraft-rs; the folder is on the user's PATH.
  3. It is uninstalled: the programs, the shortcut, the product and the PATH entry are gone.

  Changes the current user's installed programs: run it on a CI runner or a throwaway machine.

.EXAMPLE
  pwsh packaging/windows/test-msi.ps1 -Msi dist/release/lovecraft-0.2.0-windows-x64.msi `
    -Previous prosecraft-rs-0.1.2-windows-x64.msi
#>
param(
  [Parameter(Mandatory = $true)] [string] $Msi,
  [string] $Previous = ''
)
$ErrorActionPreference = 'Stop'

$Temp = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { [IO.Path]::GetTempPath() }
$Work = Join-Path $Temp "msi-test-$([IO.Path]::GetRandomFileName())"
New-Item -ItemType Directory -Force -Path $Work | Out-Null
$StartMenu = [Environment]::GetFolderPath('Programs')
$UninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall'
# The key the MSI keeps its folder under; it keeps the app's former name (lovecraft.wxs).
$AppKey = 'HKCU:\Software\ProseCraft-rs'

function Fail([string] $Message) { throw "FAIL $Message" }
function Ok([string] $Message) { Write-Output "ok   $Message" }

# The version and package name from a release file's name (prosecraft-rs-0.1.2-windows-x64.msi).
function Get-FileInfo([string] $Path) {
  $name = Split-Path -Leaf $Path
  if ($name -notmatch '^(lovecraft|prosecraft-rs)-(.+)-windows-(x64|arm64)\.msi$') { Fail "$name is not <package>-<version>-windows-<arch>.msi" }
  $display = if ($Matches[1] -eq 'lovecraft') { 'LoveCraft' } else { 'ProseCraft-rs' }
  return @{ Package = $Matches[1]; Version = $Matches[2]; Display = $display }
}

function Invoke-Msiexec([string] $Action, [string] $Path) {
  $log = Join-Path $Work "msiexec-$Action-$([IO.Path]::GetRandomFileName()).log"
  $p = Start-Process msiexec.exe -ArgumentList @($Action, "`"$Path`"", '/qn', '/norestart', '/l*v', "`"$log`"") -Wait -PassThru
  if ($p.ExitCode -ne 0) {
    if (Test-Path $log) { Get-Content $log -Tail 60 }
    Fail "msiexec $Action $(Split-Path -Leaf $Path) exited $($p.ExitCode)"
  }
  Ok "msiexec $Action $(Split-Path -Leaf $Path)"
}

# The app is a window program: its --version goes to a redirected stdout.
function Get-AppVersion([string] $Exe) {
  if (-not (Test-Path -LiteralPath $Exe)) { Fail "$Exe missing" }
  $out = Join-Path $Work "version-$([IO.Path]::GetRandomFileName()).txt"
  $p = Start-Process -FilePath $Exe -ArgumentList '--version' -Wait -PassThru -NoNewWindow -RedirectStandardOutput $out
  if ($p.ExitCode -ne 0) { Fail "$Exe --version exited $($p.ExitCode)" }
  return (Get-Content -Raw $out).Trim()
}

function Assert-Version([string] $Exe, [string] $Want) {
  $got = Get-AppVersion $Exe
  if ($got -ne $Want) { Fail "$(Split-Path -Leaf $Exe) --version said '$got', expected '$Want'" }
  Ok "$(Split-Path -Leaf $Exe) --version: $got"
}

# Without PROSECRAFT_OPEN_DIR the MCP server says so and fails; with a folder it answers.
function Assert-Mcp([string] $Exe) {
  if (-not (Test-Path -LiteralPath $Exe)) { Fail "$Exe missing" }
  $name = Split-Path -Leaf $Exe
  $env:PROSECRAFT_OPEN_DIR = $null
  $out = '' | & $Exe 2>&1 | Out-String
  if ($LASTEXITCODE -eq 0 -or $out -notmatch 'PROSECRAFT_OPEN_DIR') { Fail "$name without PROSECRAFT_OPEN_DIR: exit $LASTEXITCODE, said: $out" }
  $folder = Join-Path $Work 'mcp-folder'
  New-Item -ItemType Directory -Force -Path $folder | Out-Null
  $env:PROSECRAFT_OPEN_DIR = $folder
  $requests = @(
    '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test-msi","version":"1"}}}',
    '{"jsonrpc":"2.0","method":"notifications/initialized"}',
    '{"jsonrpc":"2.0","id":2,"method":"tools/list"}'
  )
  $out = $requests | & $Exe 2>&1 | Out-String
  $env:PROSECRAFT_OPEN_DIR = $null
  if ($out -notmatch '"serverInfo":\{' -or $out -notmatch '"tools":\[\{') { Write-Output $out; Fail "$name didn't answer initialize and tools/list" }
  Ok "$name answers initialize and tools/list"
}

function Get-Products {
  return @(Get-ChildItem $UninstallKey -ErrorAction SilentlyContinue | Get-ItemProperty |
      Where-Object { $_.PSObject.Properties['DisplayName'] -and $_.DisplayName -in @('LoveCraft', 'ProseCraft-rs') })
}

function Get-UserPath { return [Environment]::GetEnvironmentVariable('Path', 'User') }

$New = Get-FileInfo $Msi
$MsiVersion = ($New.Version -split '-')[0]
if ($New.Package -ne 'lovecraft') { Fail "$Msi is not a LoveCraft MSI" }
if (Get-Products) { Fail 'LoveCraft or ProseCraft-rs is already installed for this user' }

$PreviousDir = ''
if ($Previous) {
  $Old = Get-FileInfo $Previous
  Write-Output "--- install the previous release, $(Split-Path -Leaf $Previous)"
  Invoke-Msiexec '/i' $Previous
  $PreviousDir = (Get-ItemProperty $AppKey).InstallDir
  $oldExe = Join-Path $PreviousDir "$($Old.Package).exe"
  Assert-Version $oldExe "$($Old.Display) $($Old.Version)"
}

Write-Output "--- install $(Split-Path -Leaf $Msi)"
Invoke-Msiexec '/i' $Msi
$products = Get-Products
if ($products.Count -ne 1) { Fail "expected one installed product, found: $(($products | ForEach-Object { "$($_.DisplayName) $($_.DisplayVersion)" }) -join ', ')" }
if ($products[0].DisplayName -ne 'LoveCraft' -or $products[0].DisplayVersion -ne $MsiVersion) {
  Fail "installed: $($products[0].DisplayName) $($products[0].DisplayVersion), expected LoveCraft $MsiVersion"
}
Ok "one product installed: LoveCraft $MsiVersion$(if ($Previous) { ' (the previous one was replaced)' })"
$Dir = (Get-ItemProperty $AppKey).InstallDir
if ($PreviousDir) {
  if ($Dir.TrimEnd('\') -ne $PreviousDir.TrimEnd('\')) { Fail "installed into $Dir, not the previous version's folder $PreviousDir" }
  Ok "installed in the previous version's folder, $Dir"
} else {
  $want = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'Programs\LoveCraft'
  if ($Dir.TrimEnd('\') -ne $want) { Fail "installed into $Dir, expected $want" }
  Ok "installed in $Dir"
}
foreach ($exe in 'lovecraft.exe', 'prosecraft-rs.exe') { Assert-Version (Join-Path $Dir $exe) "LoveCraft $($New.Version)" }
foreach ($exe in 'lovecraft-mcp.exe', 'prosecraft-mcp.exe') { Assert-Mcp (Join-Path $Dir $exe) }
if (-not (Test-Path (Join-Path $StartMenu 'LoveCraft.lnk'))) { Fail "no LoveCraft shortcut in $StartMenu" }
if (Test-Path (Join-Path $StartMenu 'ProseCraft-rs.lnk')) { Fail "the ProseCraft-rs shortcut is still in $StartMenu" }
Ok 'Start menu: LoveCraft'
$pathParts = @((Get-UserPath) -split ';' | Where-Object { $_ -and ($_.TrimEnd('\') -eq $Dir.TrimEnd('\')) })
if ($pathParts.Count -ne 1) { Fail "the install folder is on the user's PATH $($pathParts.Count) times: $(Get-UserPath)" }
Ok "the install folder is on the user's PATH"

Write-Output "--- uninstall"
Invoke-Msiexec '/x' $Msi
if (Get-Products) { Fail 'still installed after uninstalling' }
foreach ($exe in 'lovecraft.exe', 'lovecraft-mcp.exe', 'prosecraft-rs.exe', 'prosecraft-mcp.exe') {
  if (Test-Path (Join-Path $Dir $exe)) { Fail "$exe left behind in $Dir" }
}
if (Test-Path (Join-Path $StartMenu 'LoveCraft.lnk')) { Fail 'the LoveCraft shortcut is left behind' }
if (@((Get-UserPath) -split ';' | Where-Object { $_ -and ($_.TrimEnd('\') -eq $Dir.TrimEnd('\')) }).Count -ne 0) { Fail "the install folder is still on the user's PATH" }
Ok 'removed: programs, shortcut, PATH entry'
Write-Output "==> all checks passed: $(Split-Path -Leaf $Msi)$(if ($Previous) { ", upgraded from $(Split-Path -Leaf $Previous)" })"
exit 0
