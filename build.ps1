# Build ssms-search-rs release binary into release\ssms-search-rs.exe
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

# Prefer WinLibs GCC if present (legacy path); otherwise use default MSVC toolchain.
$mingw = 'D:\mingw64\bin'
$wingetMingw = Join-Path $env:LOCALAPPDATA 'Microsoft\WinGet\Packages\BrechtSanders.WinLibs.POSIX.UCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64\bin'
if (Test-Path "$mingw\gcc.exe") {
    $env:Path = "$mingw;${env:USERPROFILE}\.cargo\bin;C:\Windows\System32"
    $env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = "$mingw\gcc.exe"
} elseif (Test-Path "$wingetMingw\gcc.exe") {
    $env:Path = "$wingetMingw;${env:USERPROFILE}\.cargo\bin;C:\Windows\System32"
    $env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = "$wingetMingw\gcc.exe"
}
Remove-Item Env:CC -ErrorAction SilentlyContinue
Remove-Item Env:CXX -ErrorAction SilentlyContinue
Remove-Item Env:RUSTFLAGS -ErrorAction SilentlyContinue

cargo test --lib
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo build --release
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$exe = Join-Path $PSScriptRoot 'target\release\ssms-search-rs.exe'
if (-not (Test-Path $exe)) { Write-Error "Missing $exe after build" }
$rel = Join-Path $PSScriptRoot 'release'
New-Item -ItemType Directory -Force -Path $rel | Out-Null
Copy-Item $exe (Join-Path $rel 'ssms-search-rs.exe') -Force
@"
ssms-search-rs - release build
Built: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')
Binary: release\ssms-search-rs.exe
"@ | Set-Content -Encoding UTF8 (Join-Path $rel 'README.txt')
Write-Host "OK: $rel\ssms-search-rs.exe"
