# Official Rust, installed only inside this project. No system PATH change.
$ErrorActionPreference = 'Stop'
$editorRoot = Split-Path -Parent $PSScriptRoot
$toolsDir = Join-Path $editorRoot '.tools'
New-Item -ItemType Directory -Force -Path $toolsDir | Out-Null
$installer = Join-Path $toolsDir 'rustup-init.exe'
$savedCargoHome = $env:CARGO_HOME
$savedRustupHome = $env:RUSTUP_HOME
$toolchain = Get-Content -LiteralPath (Join-Path $editorRoot 'rust-toolchain.toml') -Raw
$version = [regex]::Match($toolchain, '(?m)^channel = "([0-9.]+)"')
if (-not $version.Success) { throw 'Cannot read the pinned Rust version.' }
try {
    $env:RUSTUP_HOME = Join-Path $toolsDir 'rustup'
    $env:CARGO_HOME = Join-Path $toolsDir 'cargo'
    $rustup = Join-Path $toolsDir 'cargo/bin/rustup.exe'
    if (Test-Path -LiteralPath $rustup) {
        & $rustup toolchain install "$($version.Groups[1].Value)-x86_64-pc-windows-gnu" --profile minimal --component rustfmt,clippy,llvm-tools --target wasm32-unknown-unknown
    } else {
        Invoke-WebRequest -Uri 'https://static.rust-lang.org/rustup/archive/1.29.3/x86_64-pc-windows-msvc/rustup-init.exe' -OutFile $installer
        $hash = (Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($hash -ne '38027d51d69a5d05493627a0634dbea52d6cd793c8f072d2373d3f4163eec7db') { throw "rustup-init.exe has an unexpected hash: $hash" }
        & $installer -y --no-modify-path --default-host x86_64-pc-windows-gnu --default-toolchain $version.Groups[1].Value --profile minimal --component rustfmt,clippy,llvm-tools --target wasm32-unknown-unknown
    }
    if ($LASTEXITCODE -ne 0) { throw 'Rust installation failed.' }
} finally {
    $env:CARGO_HOME = $savedCargoHome
    $env:RUSTUP_HOME = $savedRustupHome
    if (Test-Path -LiteralPath $installer) { Remove-Item -LiteralPath $installer }
}
& (Join-Path $PSScriptRoot 'setup-native.ps1')
if ($LASTEXITCODE -ne 0) { throw 'Native tool installation failed.' }
Write-Host 'Rust is ready. Run editor/scripts/check.ps1 to verify the project.'
