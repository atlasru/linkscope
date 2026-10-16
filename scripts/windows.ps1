param([ValidateSet('Test','Build','Dev','Benchmark','Check')][string]$Task='Test')
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $projectRoot
# This machine has MinGW but no Visual C++ workload. Prefer a full GNU host so
# host proc-macros and the application use the same available native linker.
$gnuToolchain=Join-Path $env:USERPROFILE '.rustup\toolchains\stable-x86_64-pc-windows-gnu\bin'
if((Test-Path -LiteralPath (Join-Path $gnuToolchain 'cargo.exe')) -and (Test-Path -LiteralPath 'C:\msys64\mingw64\bin\gcc.exe')) {
  # MinGW must precede Rust's bin: their DLLs conflict when gcc launches cc1.
  $env:PATH="C:\msys64\mingw64\bin;$gnuToolchain;$env:PATH"
  $env:CC='C:\msys64\mingw64\bin\gcc.exe'
}
function Invoke-Checked {param([scriptblock]$Action) & $Action; if($LASTEXITCODE -ne 0){throw "Command failed with exit code $LASTEXITCODE"}}
switch($Task){
  'Test' {Invoke-Checked {npm.cmd run build};Invoke-Checked {cargo test --locked -p linkscope-engine -p linkscope-transforms};Invoke-Checked {cargo test --locked -p linkscope-desktop --features custom-protocol};Invoke-Checked {npm.cmd test};Invoke-Checked {npm.cmd run test:ui}}
  'Build' {Invoke-Checked {npm.cmd run desktop:build}}
  'Dev' {Invoke-Checked {npm.cmd run desktop}}
  'Benchmark' {Invoke-Checked {cargo run --locked --release -p linkscope-engine --example benchmark};Invoke-Checked {npm.cmd run benchmark:canvas}}
  'Check' {Invoke-Checked {npm.cmd run build};Invoke-Checked {cargo check --locked --workspace}}
}
