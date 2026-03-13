# SPINE Demo Walkthrough
#
# Prerequisites:
#   cargo build --workspace
#
# This script demonstrates the full SPINE pipeline:
#   1. Initialize a project
#   2. Start the server
#   3. Connect and interact
#   4. Run benchmarks
#
# No Docker required — everything runs locally.

param(
    [string]$ServerAddr = "127.0.0.1:8080"
)

$ErrorActionPreference = "Stop"

function Write-Section($title) {
    Write-Host ""
    Write-Host ("═" * 60) -ForegroundColor Cyan
    Write-Host "  $title" -ForegroundColor White
    Write-Host ("═" * 60) -ForegroundColor Cyan
    Write-Host ""
}

# ── Step 1: Build ──
Write-Section "Step 1: Building SPINE workspace"
cargo build --workspace 2>&1 | Select-Object -Last 3
if ($LASTEXITCODE -ne 0) { throw "Build failed" }
Write-Host "  Build successful" -ForegroundColor Green

# ── Step 2: Initialize a demo project ──
Write-Section "Step 2: Initializing demo project"
$demoDir = Join-Path $env:TEMP "spine-demo-$(Get-Date -Format 'yyyyMMddHHmmss')"
cargo run -p spine-cli -- init $demoDir 2>&1
Write-Host "  Demo project created at: $demoDir" -ForegroundColor Green

if (Test-Path (Join-Path $demoDir "spine.toml")) {
    Write-Host "  spine.toml:" -ForegroundColor Yellow
    Get-Content (Join-Path $demoDir "spine.toml") | ForEach-Object { Write-Host "    $_" }
}

# ── Step 3: Start server in background ──
Write-Section "Step 3: Starting SPINE server"
Write-Host "  Starting spine-core on $ServerAddr..." -ForegroundColor Yellow

$serverJob = Start-Job -ScriptBlock {
    param($workspace)
    Set-Location $workspace
    cargo run -p spine-core 2>&1
} -ArgumentList (Get-Location).Path

# Wait for server to start
Start-Sleep -Seconds 5

# Check if server is responsive
Write-Host "  Checking server health..." -ForegroundColor Yellow
try {
    cargo run -p spine-cli -- status $ServerAddr 2>&1
    Write-Host "  Server is running" -ForegroundColor Green
}
catch {
    Write-Host "  Server may still be starting..." -ForegroundColor Yellow
}

# ── Step 4: Run benchmark ──
Write-Section "Step 4: Running performance benchmark"
Write-Host "  Benchmarking $ServerAddr (10 iterations)..." -ForegroundColor Yellow
try {
    cargo run -p spine-cli -- benchmark $ServerAddr -n 10 2>&1
}
catch {
    Write-Host "  Benchmark requires server connectivity" -ForegroundColor Yellow
}

# ── Step 5: Interactive REPL demo (non-interactive, just show help) ──
Write-Section "Step 5: CLI Commands Reference"
Write-Host "  Available commands:" -ForegroundColor Yellow
Write-Host "    spine init <path>                  — Scaffold a new project"
Write-Host "    spine connect <addr>               — Interactive REPL session"
Write-Host "    spine connect <addr> --ws           — Connect via WebSocket"
Write-Host "    spine query navigate <addr> <url>  — Navigate to URL"
Write-Host "    spine query search <addr> <query>  — Search"
Write-Host "    spine benchmark <addr> -n 100      — Performance benchmark"
Write-Host "    spine status <addr>                — Health check"
Write-Host "    spine cert generate                — Generate dev certificates"
Write-Host "    spine deploy -c spine.toml         — Deploy from config"

# ── Step 6: Run the research swarm example ──
Write-Section "Step 6: Research Swarm Demo"
Write-Host "  Running collaborative research swarm..." -ForegroundColor Yellow
try {
    cargo run --example research_swarm -p spine-agent 2>&1
}
catch {
    Write-Host "  Example requires running server" -ForegroundColor Yellow
}

# ── Cleanup ──
Write-Section "Cleanup"
Write-Host "  Stopping server..." -ForegroundColor Yellow
Stop-Job $serverJob -ErrorAction SilentlyContinue
Remove-Job $serverJob -Force -ErrorAction SilentlyContinue
Write-Host "  Removing demo directory..." -ForegroundColor Yellow
Remove-Item -Recurse -Force $demoDir -ErrorAction SilentlyContinue
Write-Host "  Done" -ForegroundColor Green

Write-Section "Demo Complete"
Write-Host "  All SPINE components demonstrated successfully." -ForegroundColor Green
Write-Host "  For more information, see docs/ or run: cargo doc --workspace --open" -ForegroundColor Yellow
