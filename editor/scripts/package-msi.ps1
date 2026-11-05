<#
.SYNOPSIS
  Build the Windows MSI of Layerfold from the release executables and the share bundle.

.DESCRIPTION
  Stages the share bundle (licenses, notices, sources) with fresh editor/target/release effectcraft.exe and
  effectcraft-cli.exe, without portable.txt (an installed copy keeps settings in %APPDATA%). The
  bundle's dependency records cover the GUI; packages only the CLI links get their records here.
  Then builds editor/output/msi/Layerfold-<version>-windows-x64.msi with the WiX v5 CLI
  unpacked in editor/.tools/wix: wix-cli-x64.msi from a wixtoolset/wix GitHub release,
  `msiexec /a <msi> /qn TARGETDIR=<editor>\.tools\wix`, without installing it system-wide.

  Build the release executables first (cargo.ps1 --upstream build --release --locked -p effectcraft
  -p effectcraft-cli). The script refuses executables of another version.

.EXAMPLE
  powershell -NoProfile -File editor/scripts/package-msi.ps1
#>
param(
  [string] $Bundle,
  [string] $OutDir
)
$ErrorActionPreference = 'Stop'
$Editor = Split-Path $PSScriptRoot
$Root = Split-Path $Editor
if (-not $OutDir) { $OutDir = Join-Path $Editor 'output\msi' }
$Wix = Join-Path $Editor '.tools\wix\PFiles64\WiX Toolset v5.0\bin\wix.exe'
if (-not (Test-Path $Wix)) { throw "WiX v5 CLI not found at $Wix (unpack wix-cli-x64.msi v5.0.2 there with msiexec /a)" }

# The version lives in [workspace.package] of the root Cargo.toml, as in upstream packaging.
$Version = $null
$inPkg = $false
foreach ($line in Get-Content (Join-Path $Root 'Cargo.toml')) {
  if ($line -match '^\s*\[') { $inPkg = ($line.Trim() -eq '[workspace.package]'); continue }
  if ($inPkg -and $line -match '^\s*version\s*=\s*"([^"]+)"') { $Version = $Matches[1]; break }
}
if (-not $Version) { throw 'could not read [workspace.package] version from Cargo.toml' }
$MsiVersion = ($Version -split '-')[0]

if (-not $Bundle) {
  $Bundle = Join-Path $Editor "output\share-windows\EffectCraft-modified-$Version-windows-x64"
}
if (-not (Test-Path (Join-Path $Bundle 'licenses\dependencies.json'))) { throw "no share bundle with licenses at $Bundle" }

$Bin = Join-Path $Editor 'target\release' # cargo.ps1 sets CARGO_TARGET_DIR to editor/target
$Exes = @('effectcraft.exe', 'effectcraft-cli.exe')
foreach ($exe in $Exes) {
  $path = Join-Path $Bin $exe
  if (-not (Test-Path $path)) { throw "missing ${path}: build the release executables first" }
  $reported = (& $path --version | Out-String).Trim()
  if ($LASTEXITCODE -ne 0) { throw "$exe --version failed with exit code $LASTEXITCODE" }
  # The whole version token: "effectcraft 10.16.0" must not pass for 0.16.0.
  if ($reported -notmatch '^\S+ (\S+)$' -or $Matches[1] -ne $Version) { throw "$exe reports '$reported', expected version $Version" }
}
$Commit = (git -C $Root rev-parse HEAD).Trim()
$dirty = git -C $Root status --porcelain -- crates apps Cargo.toml Cargo.lock
if ($dirty) { Write-Warning "uncommitted source changes; BUILD.txt names $Commit, which may not match the executables" }

# ---- stage -------------------------------------------------------------------------------------
$Stage = Join-Path $OutDir 'stage'
if (Test-Path $Stage) { Remove-Item -Recurse -Force $Stage }
New-Item -ItemType Directory -Force $Stage | Out-Null
robocopy $Bundle $Stage /E /NFL /NDL /NJH /NJS /NP | Out-Null
if ($LASTEXITCODE -ge 8) { throw "copying the bundle failed (robocopy $LASTEXITCODE)" }
Remove-Item -Force (Join-Path $Stage 'portable.txt') -ErrorAction SilentlyContinue
foreach ($exe in $Exes) { Copy-Item -Force (Join-Path $Bin $exe) $Stage }

$utf8 = New-Object System.Text.UTF8Encoding($false)
$utf8Bom = New-Object System.Text.UTF8Encoding($true)
$readme = [IO.File]::ReadAllText((Join-Path $Editor 'packaging\windows\README-install.txt'), [Text.Encoding]::UTF8)
$readme = ($readme.Replace('{VERSION}', $Version) -replace "`r?`n", "`r`n")
[IO.File]::WriteAllText((Join-Path $Stage 'README.txt'), $readme, $utf8Bom)

# ---- dependency records for packages only the CLI links -----------------------------------------
$cargo = Join-Path $PSScriptRoot 'cargo.ps1'
function Get-Tree([string] $Package) {
  $lines = & powershell -NoProfile -File $cargo --upstream tree -p $Package -e normal,build --prefix none --target x86_64-pc-windows-gnu --offline
  if ($LASTEXITCODE -ne 0) { throw "cargo tree -p $Package failed" }
  $lines | Where-Object { $_ -match '^\S+ v\S+' } | ForEach-Object { $t = $_ -split ' '; "$($t[0]) $($t[1].TrimStart('v'))" } | Sort-Object -Unique
}
$gui = Get-Tree 'effectcraft'
$cliOnly = Get-Tree 'effectcraft-cli' | Where-Object { $gui -notcontains $_ }
$metaJson = & powershell -NoProfile -File $cargo --upstream metadata --format-version 1 --offline --filter-platform x86_64-pc-windows-gnu
if ($LASTEXITCODE -ne 0) { throw 'cargo metadata failed' }
$meta = ($metaJson | Out-String) | ConvertFrom-Json
$depsFile = Join-Path $Stage 'licenses\dependencies.json'
$records = [System.Collections.ArrayList]@(([IO.File]::ReadAllText($depsFile, [Text.Encoding]::UTF8) | ConvertFrom-Json))
$added = @()
foreach ($entry in $cliOnly) {
  $name, $ver = $entry -split ' '
  $pkg = $meta.packages | Where-Object { $_.name -eq $name -and $_.version -eq $ver } | Select-Object -First 1
  if (-not $pkg -or -not $pkg.source) { continue } # workspace packages: covered by the project's own notices
  if ($records | Where-Object { $_.name -eq $name -and $_.version -eq $ver }) { continue }
  $src = Split-Path $pkg.manifest_path
  $dest = Join-Path $Stage "licenses\dependencies\$name-$ver"
  New-Item -ItemType Directory -Force $dest | Out-Null
  $notices = @(Get-ChildItem $src -File | Where-Object { $_.Name -match '^(licen[cs]e|copying|copyright|notice|readme)' } | Sort-Object Name)
  foreach ($n in $notices) { Copy-Item $n.FullName $dest }
  $record = [ordered]@{
    name = $name; version = $ver; license = $pkg.license; authors = @($pkg.authors)
    repository = $pkg.repository; source = $pkg.source; included_notices = @($notices | ForEach-Object { $_.Name })
  }
  [IO.File]::WriteAllText((Join-Path $dest 'package.json'), ($record | ConvertTo-Json -Depth 4), $utf8)
  [void] $records.Add([pscustomobject] $record)
  $added += "$name $ver ($($pkg.license))"
}
if ($added) {
  $sorted = @($records | Sort-Object name, version)
  [IO.File]::WriteAllText($depsFile, (ConvertTo-Json -InputObject $sorted -Depth 4), $utf8)
  $licReadme = Join-Path $Stage 'licenses\README.txt'
  $text = [IO.File]::ReadAllText($licReadme, [Text.Encoding]::UTF8)
  $text = $text.Replace('cover the Windows x64 desktop executable and its', 'cover the Windows x64 desktop and command-line executables and their')
  [IO.File]::WriteAllText($licReadme, $text, $utf8)
}
Write-Output "CLI-only dependency records added: $($added.Count)"
$added | ForEach-Object { Write-Output "  $_" }

# ---- build record and checksums -----------------------------------------------------------------
$hash = @{}
foreach ($exe in $Exes) { $hash[$exe] = (Get-FileHash (Join-Path $Stage $exe) -Algorithm SHA256).Hash.ToLowerInvariant() }
$build = @(
  "Layerfold $Version, modified build (installed)",
  "Source commit: $Commit",
  'Windows x64 GNU, release ThinLTO, locked dependencies',
  "GUI SHA-256: $($hash['effectcraft.exe'])",
  "CLI SHA-256: $($hash['effectcraft-cli.exe'])"
) -join "`n"
[IO.File]::WriteAllText((Join-Path $Stage 'BUILD.txt'), "$build`n", $utf8)
$sums = ($Exes | ForEach-Object { "$($hash[$_])  $_" }) -join "`n"
[IO.File]::WriteAllText((Join-Path $Stage 'SHA256SUMS.txt'), "$sums`n", $utf8)

# ---- MSI ----------------------------------------------------------------------------------------
$Msi = Join-Path $OutDir "Layerfold-$Version-windows-x64.msi"
Remove-Item -Force $Msi -ErrorAction SilentlyContinue
$wxs = Join-Path $Editor 'packaging\windows'
& $Wix build (Join-Path $wxs 'effectcraft-modified.wxs') (Join-Path $wxs 'installer-ui.wxs') -arch x64 `
  -d "Version=$MsiVersion" -d "StageDir=$Stage" -d "IconPath=$(Join-Path $Root 'assets\app-icon\effectcraft.ico')" -o $Msi
if ($LASTEXITCODE -ne 0) { throw "wix build failed with exit code $LASTEXITCODE" }
Remove-Item -Force -ErrorAction SilentlyContinue ([IO.Path]::ChangeExtension($Msi, '.wixpdb'))
$msiHash = (Get-FileHash $Msi -Algorithm SHA256).Hash.ToLowerInvariant()
[IO.File]::WriteAllText("$Msi.sha256", "$msiHash  $(Split-Path $Msi -Leaf)`n", $utf8)
$files = (Get-ChildItem $Stage -Recurse -File).Count
Write-Output "MSI: $Msi ($((Get-Item $Msi).Length) bytes, $files files, SHA-256 $msiHash)"

# The release MSI (no -OutDir; test builds pass one) is also copied to <repo>/output, the one place
# to pick up the installer; older versions there are replaced.
if (-not $PSBoundParameters.ContainsKey('OutDir')) {
  $Pickup = Join-Path $Root 'output'
  New-Item -ItemType Directory -Force $Pickup | Out-Null
  Get-ChildItem $Pickup -Filter '*-windows-x64.msi*' | Remove-Item -Force
  Copy-Item -Force $Msi, "$Msi.sha256" $Pickup
  Write-Output "Copied to $Pickup"
}
