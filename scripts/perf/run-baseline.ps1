# PERF-11: run headless DB baseline (1k smoke; optional 10k) and write artifacts.
param(
    [switch]$Include10k,
    [string]$ArtifactRoot = ""
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

Write-Host "Running PERF-11 1k smoke into $ArtifactRoot"
cargo test -j 1 --manifest-path src-tauri/Cargo.toml --test perf_baseline -- --nocapture perf_baseline_1k_smoke
if ($LASTEXITCODE -ne 0) {
    throw "perf_baseline_1k_smoke failed with exit $LASTEXITCODE"
}

if ($Include10k) {
    Write-Host "Running PERF-11 10k ignored baseline"
    cargo test -j 1 --manifest-path src-tauri/Cargo.toml --test perf_baseline -- --ignored --nocapture perf_baseline_10k
    if ($LASTEXITCODE -ne 0) {
        throw "perf_baseline_10k failed with exit $LASTEXITCODE"
    }
}

Write-Host "PERF-11 artifacts ready under $ArtifactRoot"
Get-ChildItem $ArtifactRoot | ForEach-Object { Write-Host " - $($_.Name)" }
