# Stop on the first failure. --locked prevents silent dependency upgrades.
$ErrorActionPreference = 'Stop'
& (Join-Path $PSScriptRoot 'doctor.ps1')
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
& (Join-Path $PSScriptRoot 'cargo.ps1') fmt --all '--' --check
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
& (Join-Path $PSScriptRoot 'cargo.ps1') clippy --workspace --all-targets --locked '--' -D warnings
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
& (Join-Path $PSScriptRoot 'cargo.ps1') test --workspace --locked
exit $LASTEXITCODE
