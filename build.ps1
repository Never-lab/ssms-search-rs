# Build ssms-search-rs with WinLibs GCC (path must not contain spaces).
$ErrorActionPreference = 'Stop'
$mingw = 'D:\mingw64\bin'
if (-not (Test-Path "$mingw\gcc.exe")) {
    Write-Error "Missing $mingw\gcc.exe — copy WinLibs mingw64 to D:\mingw64 first."
}
$env:Path = "$mingw;$env:USERPROFILE\.cargo\bin;C:\Windows\System32"
Remove-Item Env:CC -ErrorAction SilentlyContinue
Remove-Item Env:CXX -ErrorAction SilentlyContinue
$env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = "$mingw\gcc.exe"
Set-Location $PSScriptRoot
# Stale RUSTFLAGS (e.g. linker=rust-lld) break mingw; always clear for this script.
Remove-Item Env:RUSTFLAGS -ErrorAction SilentlyContinue
cargo test
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo build --release
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$exe = Join-Path $PSScriptRoot 'target\release\ssms-search-rs.exe'
if (-not (Test-Path $exe)) { Write-Error "Missing $exe after build" }
$rel = Join-Path $PSScriptRoot 'release'
New-Item -ItemType Directory -Force -Path $rel | Out-Null
Copy-Item $exe (Join-Path $rel 'ssms-search-rs.exe') -Force
@"
ssms-search-rs — release build
Built: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')
Binary: release\ssms-search-rs.exe
"@ | Set-Content -Encoding UTF8 (Join-Path $rel 'README.txt')
Write-Host "OK: $rel\ssms-search-rs.exe"
