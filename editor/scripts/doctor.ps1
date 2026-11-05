# Read-only local diagnostics. No downloads, installations or Git changes.
$ErrorActionPreference = 'Stop'
$editorRoot = Split-Path -Parent $PSScriptRoot
$projectRoot = Split-Path -Parent $editorRoot
$failed = $false
function Report([string]$Label, [bool]$Ok, [string]$Detail) {
    if (-not $Ok) { $script:failed = $true }
    $status = if ($Ok) { 'OK' } else { 'FAIL' }
    Write-Host "[$status] ${Label}: $Detail"
}
Report 'Workspace' (Test-Path (Join-Path $projectRoot 'crates/engine/Cargo.toml')) $projectRoot
$git = Get-Command git -ErrorAction SilentlyContinue
Report 'Git' ($null -ne $git) 'Required for source history and updates'
if ($git) {
    & git -C $projectRoot rev-parse --show-toplevel
    Report 'Git checkout' ($LASTEXITCODE -eq 0) 'Primary checkout'
    $head = & git -C $projectRoot rev-parse HEAD
    Write-Host "Local HEAD: $head (may include our future commits)"
}
$baseline = Get-Content -LiteralPath (Join-Path $editorRoot 'upstream.toml') -Raw
$baselineMatch = [regex]::Match($baseline, '(?m)^revision = "([0-9a-f]{40})"')
Report 'Pinned upstream' $baselineMatch.Success $baselineMatch.Groups[1].Value
Report 'Lockfile' (Test-Path (Join-Path $editorRoot 'Cargo.lock')) 'editor/Cargo.lock'
$nativeManifest = Get-Content -LiteralPath (Join-Path $editorRoot 'native-tools.json') -Raw | ConvertFrom-Json
$nativeRoot = Join-Path $editorRoot ".tools/$($nativeManifest.directory)"
Report 'Windows resources' (Test-Path -LiteralPath (Join-Path $nativeRoot 'bin/x86_64-w64-mingw32-windres.exe')) $nativeManifest.directory
$toolchain = Get-Content -LiteralPath (Join-Path $editorRoot 'rust-toolchain.toml') -Raw
$version = [regex]::Match($toolchain, '(?m)^channel = "([0-9.]+)"')
$importLibrary = Join-Path $editorRoot ".tools/rustup/toolchains/$($version.Groups[1].Value)-x86_64-pc-windows-gnu/lib/rustlib/x86_64-pc-windows-gnu/lib/self-contained/libshlwapi.a"
Report 'Windows import library' (Test-Path -LiteralPath $importLibrary) 'shlwapi (required by the upstream UI)'
& (Join-Path $PSScriptRoot 'cargo.ps1') --version
Report 'Cargo' ($LASTEXITCODE -eq 0) 'Pinned project toolchain'
& (Join-Path $PSScriptRoot 'cargo.ps1') metadata --no-deps --format-version 1 --offline | Out-Null
Report 'Workspace manifests' ($LASTEXITCODE -eq 0) 'No dependency downloads requested'
if ($failed) { exit 1 }
exit 0
