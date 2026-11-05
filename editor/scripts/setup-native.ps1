# llvm-ar includes the dlltool driver and selects it by its executable name.
# https://github.com/llvm/llvm-project/blob/main/llvm/tools/llvm-ar/CMakeLists.txt
$ErrorActionPreference = 'Stop'
$editorRoot = Split-Path -Parent $PSScriptRoot
$toolsDir = Join-Path $editorRoot '.tools'
$toolchain = Get-Content -LiteralPath (Join-Path $editorRoot 'rust-toolchain.toml') -Raw
$version = [regex]::Match($toolchain, '(?m)^channel = "([0-9.]+)"')
if (-not $version.Success) { throw 'Cannot read the pinned Rust version.' }
$llvmBin = Join-Path $editorRoot ".tools/rustup/toolchains/$($version.Groups[1].Value)-x86_64-pc-windows-gnu/lib/rustlib/x86_64-pc-windows-gnu/bin"
$source = Join-Path $llvmBin 'llvm-ar.exe'
if (-not (Test-Path -LiteralPath $source)) { throw 'Install the llvm-tools Rust component via setup.ps1.' }
$aliasBin = Join-Path $editorRoot '.tools/bin'
New-Item -ItemType Directory -Force -Path $aliasBin | Out-Null
Copy-Item -LiteralPath $source -Destination (Join-Path $aliasBin 'dlltool.exe') -Force
$savedPath = $env:PATH
try {
    $env:PATH = "${llvmBin};$env:PATH"
    $helpText = & (Join-Path $aliasBin 'dlltool.exe') --help | Out-String
    # This LLVM driver returns 1 for help (no input file); verify it actually started.
    if ($LASTEXITCODE -notin @(0, 1) -or $helpText -notmatch 'OVERVIEW: llvm-dlltool') {
        throw 'The LLVM dlltool driver could not start.'
    }
} finally {
    $env:PATH = $savedPath
}
Write-Host 'LLVM dlltool is ready for paths containing spaces.'

# Rust's bundled MinGW libraries omit shlwapi, which webbrowser (and therefore eframe)
# links. Use the official LLVM-MinGW package for this import library and Windows resources.
# Keep Rust's linker and CRT: replacing them with LLVM-MinGW's CRT would mix runtimes.
$manifest = Get-Content -LiteralPath (Join-Path $editorRoot 'native-tools.json') -Raw | ConvertFrom-Json
if ($manifest.directory -notmatch '^llvm-mingw-[0-9]{8}-msvcrt-x86_64$' -or $manifest.sha256 -notmatch '^[0-9a-f]{64}$') {
    throw 'Invalid native-tools.json package name or checksum.'
}
$nativeRoot = Join-Path $toolsDir $manifest.directory
$readyFile = Join-Path $nativeRoot '.installed-sha256'
$importLibrary = Join-Path $nativeRoot 'x86_64-w64-mingw32/lib/libshlwapi.a'
$windres = Join-Path $nativeRoot 'bin/x86_64-w64-mingw32-windres.exe'
$requiredFiles = @($importLibrary, $windres, (Join-Path $nativeRoot 'bin/llvm-rc.exe'))
$ready = (Test-Path -LiteralPath $readyFile) -and ((Get-Content -LiteralPath $readyFile -Raw).Trim() -eq $manifest.sha256)
foreach ($required in $requiredFiles) { $ready = $ready -and (Test-Path -LiteralPath $required) }
if (-not $ready) {
    $archive = Join-Path $toolsDir "$($manifest.directory).zip"
    try {
        Write-Host 'Downloading the pinned LLVM-MinGW tools (about 165 MiB)...'
        Invoke-WebRequest -Uri $manifest.url -OutFile $archive
        if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ne $manifest.sha256) {
            throw 'LLVM-MinGW checksum mismatch. The archive will not be unpacked.'
        }
        Expand-Archive -LiteralPath $archive -DestinationPath $toolsDir -Force
    } finally {
        if (Test-Path -LiteralPath $archive) { Remove-Item -LiteralPath $archive }
    }
}
foreach ($required in $requiredFiles) {
    if (-not (Test-Path -LiteralPath $required)) { throw "LLVM-MinGW installation is incomplete: $required" }
}
# Add only the missing system import library to the project-local sysroot's existing search
# path. No global Rust flags, no replacement CRT libraries and no changes to Cargo dependencies.
$sysrootLib = Join-Path (Split-Path -Parent $llvmBin) 'lib/self-contained'
Copy-Item -LiteralPath $importLibrary -Destination (Join-Path $sysrootLib 'libshlwapi.a') -Force
& $windres --version
if ($LASTEXITCODE -ne 0) { throw 'The Windows resource compiler could not start.' }
Set-Content -LiteralPath $readyFile -Value $manifest.sha256 -Encoding Ascii
Write-Host 'Windows import library and resource compiler are ready.'
exit 0
