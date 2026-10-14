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
cargo test
cargo build --release
$exe = Join-Path $PSScriptRoot 'target\release\ssms-search-rs.exe'
$rel = Join-Path $PSScriptRoot 'release'
New-Item -ItemType Directory -Force -Path $rel | Out-Null
Copy-Item $exe (Join-Path $rel 'ssms-search-rs.exe') -Force
@"
ssms-search-rs — release build
Built: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')
Binary: release\ssms-search-rs.exe
"@ | Set-Content -Encoding UTF8 (Join-Path $rel 'README.txt')
Write-Host "OK: $rel\ssms-search-rs.exe"
