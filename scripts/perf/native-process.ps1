param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('Launch', 'Sample', 'Check', 'VerifyExit')]
    [string]$Mode,
    [Parameter(Mandatory = $true)]
    [string]$Executable,
    [int]$CandidatePid = 0,
    [ValidateRange(1024, 65535)]
    [int]$Port = 19424,
    [string]$OwnedProcessIds = ''
)

$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$candidatePath = (Resolve-Path -LiteralPath $Executable).Path
$processes = @(Get-CimInstance Win32_Process)
$matching = @($processes | Where-Object { $_.ExecutablePath -eq $candidatePath })

if ($Mode -eq 'VerifyExit') {
    if ($OwnedProcessIds -notmatch '^\d+(,\d+){0,127}$') { throw 'Invalid candidate process snapshot.' }
    $expectedIds = @($OwnedProcessIds.Split(',') | ForEach-Object { [int]$_ })
    @{ remaining = @($processes | Where-Object { $expectedIds -contains [int]$_.ProcessId } | ForEach-Object { [int]$_.ProcessId }) } | ConvertTo-Json -Compress
    exit 0
}

if ($Mode -eq 'Check') {
    @{ running = $matching.Count -gt 0 } | ConvertTo-Json -Compress
    exit 0
}

if ($Mode -eq 'Launch') {
    if ($matching.Count -gt 0) { throw 'The candidate executable is already running.' }
    $oldArguments = $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS
    try {
        $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$Port --remote-debugging-address=127.0.0.1"
        $startedAtUtc = (Get-Date).ToUniversalTime().ToString('o')
        $launchClock = [System.Diagnostics.Stopwatch]::StartNew()
        $candidate = Start-Process -FilePath $candidatePath -WorkingDirectory (Split-Path -Parent $candidatePath) -WindowStyle Hidden -PassThru
        @{ pid = $candidate.Id; startedAtUtc = $startedAtUtc; launchCallMs = $launchClock.Elapsed.TotalMilliseconds } | ConvertTo-Json -Compress
    } finally {
        $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $oldArguments
    }
    exit 0
}

$mainProcess = $processes | Where-Object { $_.ProcessId -eq $CandidatePid } | Select-Object -First 1
if (!$mainProcess) {
    @{ alive = $false; processes = @() } | ConvertTo-Json -Compress
    exit 0
}
if ($mainProcess.ExecutablePath -ne $candidatePath) { throw 'Candidate PID was reused by another executable.' }

$ownedIds = [System.Collections.Generic.HashSet[int]]::new()
[void]$ownedIds.Add($CandidatePid)
do {
    $added = $false
    foreach ($entry in $processes) {
        if ($ownedIds.Contains([int]$entry.ParentProcessId) -and $ownedIds.Add([int]$entry.ProcessId)) { $added = $true }
    }
} while ($added)

$samples = @(
    foreach ($ownedId in $ownedIds) {
        $entry = Get-Process -Id $ownedId -ErrorAction SilentlyContinue
        if ($entry) {
            $nativeCounters = $processes | Where-Object { $_.ProcessId -eq $ownedId } | Select-Object -First 1
            [PSCustomObject][ordered]@{
                pid = $entry.Id
                name = $entry.ProcessName
                workingSetBytes = [long]$entry.WorkingSet64
                privateBytes = [long]$entry.PrivateMemorySize64
                cpuSeconds = $entry.CPU
                handles = $entry.HandleCount
                threads = $entry.Threads.Count
                readTransferBytes = [long]$nativeCounters.ReadTransferCount
                writeTransferBytes = [long]$nativeCounters.WriteTransferCount
            }
        }
    }
)
[ordered]@{
    alive = $true
    collectedAt = (Get-Date).ToUniversalTime().ToString('o')
    processes = $samples
    workingSetBytes = ($samples | Measure-Object -Property workingSetBytes -Sum).Sum
    privateBytes = ($samples | Measure-Object -Property privateBytes -Sum).Sum
    cpuSeconds = ($samples | Measure-Object -Property cpuSeconds -Sum).Sum
    handles = ($samples | Measure-Object -Property handles -Sum).Sum
    threads = ($samples | Measure-Object -Property threads -Sum).Sum
    readTransferBytes = ($samples | Measure-Object -Property readTransferBytes -Sum).Sum
    writeTransferBytes = ($samples | Measure-Object -Property writeTransferBytes -Sum).Sum
} | ConvertTo-Json -Depth 5 -Compress
