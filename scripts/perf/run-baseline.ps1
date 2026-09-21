# PERF-11 / PERF-01: run headless DB baseline (1k smoke; optional 10k / 50k) and write artifacts.
param(
    [switch]$Include10k,
    [switch]$Include50k,
    [string]$ArtifactRoot = "",
    # PERF-09: build the release binary under `opt-level="s"` and `"3"` and
    # record build time plus binary size. Off by default: the throughput
    # comparison itself is still unscheduled (see audit PERF-09).
    [switch]$CompareOptLevel
)

$ErrorActionPreference = "Stop"
$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..\..")
Set-Location $repoRoot

if (-not $ArtifactRoot) {
    $ts = Get-Date -Format "yyyyMMdd-HHmmss"
    $ArtifactRoot = Join-Path $repoRoot "artifacts\perf\$ts"
}

New-Item -ItemType Directory -Force -Path $ArtifactRoot | Out-Null
& "$PSScriptRoot\collect-metadata.ps1" -OutDir $ArtifactRoot

$env:VIBE_PERF_ARTIFACT_DIR = $ArtifactRoot
$env:VIBE_PERF_REPS = if ($env:VIBE_PERF_REPS) { $env:VIBE_PERF_REPS } else { "5" }

# ENG-03: -j 1 never fixed test interference (it limits compile parallelism,
# not test threads); the real local constraint is linker memory. -j 2 is the
# documented setting and matches README / ROADMAP.
Write-Host "Running PERF-11 1k smoke into $ArtifactRoot"
cargo test -j 2 --manifest-path src-tauri/Cargo.toml --test perf_baseline -- --nocapture perf_baseline_1k_smoke
if ($LASTEXITCODE -ne 0) {
    throw "perf_baseline_1k_smoke failed with exit $LASTEXITCODE"
}

if ($Include10k) {
    Write-Host "Running PERF-11 10k ignored baseline"
    cargo test -j 2 --manifest-path src-tauri/Cargo.toml --test perf_baseline -- --ignored --nocapture perf_baseline_10k
    if ($LASTEXITCODE -ne 0) {
        throw "perf_baseline_10k failed with exit $LASTEXITCODE"
    }
}

if ($Include50k) {
    Write-Host "Running PERF-01 50k ignored baseline"
    cargo test -j 2 --manifest-path src-tauri/Cargo.toml --test perf_baseline -- --ignored --nocapture perf_baseline_50k
    if ($LASTEXITCODE -ne 0) {
        throw "perf_baseline_50k failed with exit $LASTEXITCODE"
    }
}

if ($CompareOptLevel) {
    # PERF-09 scaffolding: the harness above only runs in the debug profile
    # (perf_baseline.rs is `#![cfg(debug_assertions)]`), so the s-vs-3 question
    # needs its own release builds. This records build cost and binary size per
    # opt-level; hash/AES/XML/BT/下载 throughput still have to be measured by
    # hand or by a dedicated microbench before PERF-09 can be Closed.
    $optRoot = Join-Path $ArtifactRoot "opt-level"
    New-Item -ItemType Directory -Force -Path $optRoot | Out-Null
    $exe = Join-Path $repoRoot "src-tauri\target\release\vibe-downloader.exe"

    foreach ($level in @("s", "3")) {
        Write-Host "PERF-09: release build with opt-level=$level"
        $env:CARGO_PROFILE_RELEASE_OPT_LEVEL = $level
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        cargo build --release --manifest-path src-tauri/Cargo.toml
        $buildFailed = $LASTEXITCODE
        $sw.Stop()
        Remove-Item Env:CARGO_PROFILE_RELEASE_OPT_LEVEL -ErrorAction SilentlyContinue
        if ($buildFailed -ne 0) {
            throw "release build with opt-level=$level failed with exit $buildFailed"
        }

        $sizeBytes = if (Test-Path $exe) { (Get-Item $exe).Length } else { 0 }
        [ordered]@{
            optLevel     = $level
            buildSeconds = [math]::Round($sw.Elapsed.TotalSeconds, 1)
            binaryBytes  = $sizeBytes
            binaryPath   = $exe
            collectedAt  = (Get-Date).ToUniversalTime().ToString("o")
        } | ConvertTo-Json | Set-Content (Join-Path $optRoot "opt-level-$level.json") -Encoding utf8
    }
    Write-Host "PERF-09 scaffolding wrote $optRoot; fill docs/performance-baseline-results.md from it"
}

Write-Host "PERF baseline artifacts ready under $ArtifactRoot"
Get-ChildItem $ArtifactRoot | ForEach-Object { Write-Host " - $($_.Name)" }
