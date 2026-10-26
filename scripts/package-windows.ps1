$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)

$metadata = python scripts/release.py metadata | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { throw 'Invalid release metadata' }

# Ship the CRT inside the binary so clean machines need no redistributable installer.
$previousFlags = $env:RUSTFLAGS
try {
    $env:RUSTFLAGS = "$previousFlags -C target-feature=+crt-static".Trim()
    cargo build --locked --release --target x86_64-pc-windows-msvc
    if ($LASTEXITCODE -ne 0) { throw 'Release build failed' }
} finally {
    $env:RUSTFLAGS = $previousFlags
}
python scripts/release.py licenses target/windows-licenses
if ($LASTEXITCODE -ne 0) { throw 'License staging failed' }

$compiler = Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe'
if (-not (Test-Path $compiler)) { throw 'Install Inno Setup 6 before packaging' }
& $compiler "/DDostyRoot=$PWD" "/DDostyVersion=$($metadata.version)" "/DDostyWindowsVersion=$($metadata.windows_version)" packaging/windows/dosty.iss
if ($LASTEXITCODE -ne 0) { throw 'Installer build failed' }
