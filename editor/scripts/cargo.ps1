# Pass --upstream as the first argument to use the original EffectCraft workspace.
# Quote a literal separator in PowerShell: cargo.ps1 run -p example '--' --flag.
$ErrorActionPreference = 'Stop'
$editorRoot = Split-Path -Parent $PSScriptRoot
$workingRoot = $editorRoot
$cargoArguments = @($args)
if ($cargoArguments.Count -gt 0 -and $cargoArguments[0] -eq '--upstream') {
    $workingRoot = Split-Path -Parent $editorRoot
    $cargoArguments = @($cargoArguments | Select-Object -Skip 1)
}
$portableCargo = Join-Path $editorRoot '.tools/cargo/bin/cargo.exe'
$environmentNames = @('CARGO_HOME', 'RUSTUP_HOME', 'PATH', 'CARGO_TARGET_DIR', 'RUSTUP_TOOLCHAIN', 'CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER', 'WINDRES', 'AR', 'EFFECTCRAFT_REQUIRE_WINRES', 'CARGO_PROFILE_DEV_DEBUG', 'EFFECTCRAFT_SYSTEM_LOCALE', 'CARGO_ENCODED_RUSTFLAGS')
$savedEnvironment = @{}
foreach ($name in $environmentNames) { $savedEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }
$cargoResult = 1
# Build caches only grow when flags, toolchains or dependencies change, so warn early instead of
# letting a full disk fail a build halfway. Measuring 36k files takes about 0.2 s. Warnings go to
# stderr: `powershell -File` prints the warning stream to stdout, which breaks callers that parse
# cargo's stdout (the `cargo metadata` JSON in package-msi.ps1).
try {
    $targetRoot = Join-Path $editorRoot 'target'
    foreach ($limit in @(@('debug', 40), @('release', 60))) {
        $dir = New-Object IO.DirectoryInfo (Join-Path $targetRoot $limit[0])
        if (-not $dir.Exists) { continue }
        $bytes = 0
        foreach ($file in $dir.EnumerateFiles('*', [IO.SearchOption]::AllDirectories)) { $bytes += $file.Length }
        if ($bytes / 1GB -gt $limit[1]) {
            [Console]::Error.WriteLine("WARNING: " + ("editor/target/{0} is {1:N0} GB (limit {2} GB). It is a rebuildable cache: delete the folder to free space." -f $limit[0], ($bytes / 1GB), $limit[1]))
        }
    }
    $drive = New-Object IO.DriveInfo ([IO.Path]::GetPathRoot($editorRoot))
    if ($drive.AvailableFreeSpace / 1GB -lt 50) {
        [Console]::Error.WriteLine("WARNING: " + ("Only {0:N0} GB free on {1}. Builds need tens of GB; delete editor/target/debug if it is large." -f ($drive.AvailableFreeSpace / 1GB), $drive.Name))
    }
} catch {
    [Console]::Error.WriteLine("WARNING: Cache size check skipped: $($_.Exception.Message)")
}
Push-Location $workingRoot
try {
    if (-not (Test-Path -LiteralPath $portableCargo)) {
        throw 'Project-local Rust is missing. Run editor/scripts/setup.ps1 first.'
    }
    $env:CARGO_HOME = Join-Path $editorRoot '.tools/cargo'
    $env:RUSTUP_HOME = Join-Path $editorRoot '.tools/rustup'
    $env:CARGO_TARGET_DIR = Join-Path $editorRoot 'target'
    $toolchain = Get-Content -LiteralPath (Join-Path $editorRoot 'rust-toolchain.toml') -Raw
    $version = [regex]::Match($toolchain, '(?m)^channel = "([0-9.]+)"')
    if (-not $version.Success) { throw 'Cannot read the pinned Rust version.' }
    $env:RUSTUP_TOOLCHAIN = "$($version.Groups[1].Value)-x86_64-pc-windows-gnu"
    # Pair LLVM-generated import libraries with LLD. The bundled GNU linker produced a
    # pre-main access violation for this dependency graph; the integration check covers it.
    $env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = 'rust-lld.exe'
    # LLVM's dlltool driver handles spaces in paths; GNU dlltool splits its temporary paths.
    $llvmBin = Join-Path $env:RUSTUP_HOME "toolchains/$env:RUSTUP_TOOLCHAIN/lib/rustlib/x86_64-pc-windows-gnu/bin"
    $nativeTools = Join-Path $llvmBin 'self-contained'
    $aliasBin = Join-Path $editorRoot '.tools/bin'
    if (-not (Test-Path (Join-Path $aliasBin 'dlltool.exe'))) {
        throw 'The LLVM dlltool alias is missing. Run editor/scripts/setup.ps1.'
    }
    $nativeManifest = Get-Content -LiteralPath (Join-Path $editorRoot 'native-tools.json') -Raw | ConvertFrom-Json
    $resourceCompiler = Join-Path $editorRoot ".tools/$($nativeManifest.directory)/bin/x86_64-w64-mingw32-windres.exe"
    $importLibrary = Join-Path (Split-Path -Parent $llvmBin) 'lib/self-contained/libshlwapi.a'
    if (-not (Test-Path -LiteralPath $resourceCompiler) -or -not (Test-Path -LiteralPath $importLibrary)) {
        throw 'Windows build support is missing. Run editor/scripts/setup.ps1.'
    }
    $env:WINDRES = $resourceCompiler
    $env:AR = Join-Path $llvmBin 'llvm-ar.exe'
    # A successful upstream build must include its icon/version resources, not silently skip them.
    $env:EFFECTCRAFT_REQUIRE_WINRES = '1'
    # Debug info made every test binary 400-500 MB of its ~550 MB and the dev cache passed 120 GB;
    # panic messages keep file:line without it (Location). A caller's own value still wins.
    if ($null -eq $savedEnvironment['CARGO_PROFILE_DEV_DEBUG']) { $env:CARGO_PROFILE_DEV_DEBUG = '0' }
    # Upstream tests expect English labels; Match System would follow the machine's language.
    if ($null -eq $savedEnvironment['EFFECTCRAFT_SYSTEM_LOCALE'] -and $cargoArguments.Count -gt 0 -and $cargoArguments[0] -in @('test', 'xtask', 'nextest')) {
        $env:EFFECTCRAFT_SYSTEM_LOCALE = 'en'
    }
    # Panic locations name each source file by the folder it was compiled in, and a release build
    # goes to other people. The last matching prefix wins, so the narrower one comes last. Dev
    # builds keep real paths: changing these flags rebuilds everything.
    $isRelease = ($cargoArguments -join ' ') -match '(^| )(-r|--release|--profile[ =]release)( |$)'
    if ($isRelease -and $null -ne $savedEnvironment['CARGO_ENCODED_RUSTFLAGS']) {
        [Console]::Error.WriteLine('WARNING: CARGO_ENCODED_RUSTFLAGS is set, so this release build keeps its folder paths.')
    } elseif ($isRelease) {
        $sourceRoot = [IO.Path]::GetFullPath((Split-Path -Parent $editorRoot))
        $cargoHome = [IO.Path]::GetFullPath($env:CARGO_HOME)
        $env:CARGO_ENCODED_RUSTFLAGS = "--remap-path-prefix=$sourceRoot=/src/layerfold$([char]0x1f)--remap-path-prefix=$cargoHome=/cargo"
    }
    $env:PATH = "$(Split-Path -Parent $portableCargo);${aliasBin};${llvmBin};${nativeTools};$env:PATH"
    & $portableCargo @cargoArguments
    $cargoResult = $LASTEXITCODE
} finally {
    Pop-Location
    foreach ($name in $environmentNames) {
        if ($null -eq $savedEnvironment[$name]) {
            Remove-Item -LiteralPath "Env:$name" -ErrorAction SilentlyContinue
        } else {
            [Environment]::SetEnvironmentVariable($name, $savedEnvironment[$name], 'Process')
        }
    }
}
exit $cargoResult
