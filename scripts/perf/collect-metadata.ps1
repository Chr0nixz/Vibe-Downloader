# Collect host/build metadata for PERF-11 baseline artifacts.
param(
    [Parameter(Mandatory = $true)]
    [string]$OutDir
)

$ErrorActionPreference = "Stop"

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

function Get-GitCommit {
    try {
        return (git -C (Resolve-Path "$PSScriptRoot\..\..") rev-parse HEAD).Trim()
    } catch {
        return "unknown"
    }
}

function Get-GitDirty {
    try {
        $status = git -C (Resolve-Path "$PSScriptRoot\..\..") status --porcelain
        return [bool]$status
    } catch {
        return $false
    }
}

$os = Get-CimInstance Win32_OperatingSystem
$cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
$rustc = try { (rustc --version).Trim() } catch { "unknown" }
$cargo = try { (cargo --version).Trim() } catch { "unknown" }
$node = try { (node --version).Trim() } catch { "unknown" }

$meta = [ordered]@{
    schemaVersion = 1
    collectedAt   = (Get-Date).ToUniversalTime().ToString("o")
    gitCommit     = Get-GitCommit
    gitDirty      = Get-GitDirty
    appVersion    = "0.3.0"
    os            = [ordered]@{
        platform     = "windows"
        caption      = $os.Caption
        version      = $os.Version
        buildNumber  = $os.BuildNumber
        architecture = $env:PROCESSOR_ARCHITECTURE
    }
    cpu = [ordered]@{
        name  = $cpu.Name
        cores = $cpu.NumberOfCores
        logicalProcessors = $cpu.NumberOfLogicalProcessors
    }
    ram = [ordered]@{
        totalBytes = [int64]$os.TotalVisibleMemorySize * 1024
        freeBytes  = [int64]$os.FreePhysicalMemory * 1024
    }
    toolchain = [ordered]@{
        rustc = $rustc
        cargo = $cargo
        node  = $node
    }
    buildProfile = "debug (cargo test)"
}

$path = Join-Path $OutDir "metadata.json"
$meta | ConvertTo-Json -Depth 6 | Set-Content -Path $path -Encoding utf8
Write-Host "Wrote $path"
