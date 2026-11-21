# SPINE Benchmark Suite — PowerShell version
#
# Usage:
#   .\scripts\bench.ps1             # Run all benchmarks
#   .\scripts\bench.ps1 -Save       # Run and save baseline
#   .\scripts\bench.ps1 -Compare    # Compare against saved baseline

param(
    [switch]$Save,
    [switch]$Compare
)

$ErrorActionPreference = "Stop"
$BaselineName = "baseline"

if ($Save) {
    Write-Host "═══ Running benchmarks and saving as baseline ═══" -ForegroundColor Cyan
    cargo bench --workspace -- --save-baseline $BaselineName
    Write-Host "Baseline saved as '$BaselineName'" -ForegroundColor Green
}
elseif ($Compare) {
    Write-Host "═══ Running benchmarks and comparing against baseline ═══" -ForegroundColor Cyan
    cargo bench --workspace -- --baseline $BaselineName
}
else {
    Write-Host "═══ SPINE Performance Benchmark Suite ═══" -ForegroundColor Cyan
    Write-Host ""

    Write-Host "Running workspace-level hot-path benchmarks..." -ForegroundColor Yellow
    cargo bench --bench hot_path_bench
    Write-Host ""

    Write-Host "Running kernel benchmarks..." -ForegroundColor Yellow
    Push-Location research; cargo bench -p spine-kernel --bench kernel_bench; Pop-Location
    Write-Host ""

    Write-Host "Running scalability benchmarks..." -ForegroundColor Yellow
    Push-Location research; cargo bench -p spine-agentic --bench scalability_bench; Pop-Location
    Write-Host ""

    Write-Host "═══ Reports written to target/criterion/ ═══" -ForegroundColor Green
    Write-Host "Open target/criterion/report/index.html in a browser"
}
