# Boot time as a user experiences it: no debugging port attached.
#
# `boot-bench.ps1` measures through CDP, and CDP roughly DOUBLES WebView2
# startup. Every boot figure this project recorded that way described a boot
# nobody experiences -- the long-standing "~930 ms" was measurement distortion,
# and the real number was 484 ms. Keeping only the CDP benchmark meant the
# honest figure could not be reproduced without hand-running the app.
#
# So this reads the app's own `ui_ready` log line instead, which is emitted
# after the first project rows paint. The binary is started with stdout
# redirected to a file; a windows-subsystem process still writes to a handle it
# is given, so no console is needed and nothing about the run is instrumented.
#
#   powershell -ExecutionPolicy Bypass -File tools\verify\boot-real.ps1
#   powershell -ExecutionPolicy Bypass -File tools\verify\boot-real.ps1 -Runs 10

param(
    [int]$Runs = 7,
    [string]$Exe = "target\release\launch-deck.exe"
)

$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..\..")

if (-not (Test-Path $Exe)) {
    Write-Output "FAIL  no release build at $Exe"
    Write-Output "      build with: npx tauri build --no-bundle"
    exit 1
}

$outFile = Join-Path $env:TEMP "launch-deck-boot.out"
$errFile = Join-Path $env:TEMP "launch-deck-boot.err"

# Report the machine's load with the result, always.
#
# Most of this number is WebView2 starting a browser engine, which is entirely
# at the mercy of whatever else wants the CPU. A "721 ms regression" was chased
# once that turned out to be background cargo builds at 98%, and a later
# comparison was quietly skewed by a game holding 49%. A boot figure without
# its load is not a measurement, it is an anecdote -- so the load is printed
# next to it and a loud warning appears past 25%.
$load = [int](Get-CimInstance Win32_PerfFormattedData_PerfOS_Processor -Filter "Name='_Total'").PercentProcessorTime
$busiest = (Get-Process | Sort-Object CPU -Descending | Select-Object -First 1).Name

# A debugging port left in the environment by an earlier harness would silently
# reintroduce exactly the distortion this script exists to avoid.
if ($env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS) {
    Write-Output "note: clearing WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS for this run"
    $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = ""
}
$env:RUST_LOG = "info"

$totals = @()
$phases = @()

foreach ($i in 1..$Runs) {
    Get-Process launch-deck -ErrorAction SilentlyContinue | ForEach-Object {
        $_.Kill(); $_.WaitForExit(6000) | Out-Null
    }
    Start-Sleep -Milliseconds 900

    if (Test-Path $outFile) { Clear-Content $outFile }

    Start-Process -FilePath $Exe -RedirectStandardOutput $outFile -RedirectStandardError $errFile
    $found = $null
    foreach ($w in 1..150) {
        Start-Sleep -Milliseconds 100
        if (Test-Path $outFile) {
            $found = Select-String -Path $outFile -Pattern "ui ready" -ErrorAction SilentlyContinue |
                     Select-Object -First 1
            if ($found) { break }
        }
    }

    if (-not $found) { Write-Output ("run {0,2}: no ui_ready mark within 15s" -f $i); continue }

    $line = $found.Line
    $total = [regex]::Match($line, 'ms=(\d+)').Groups[1].Value
    $dom = [regex]::Match($line, 'dom_interactive=(\d+)').Groups[1].Value
    $painted = [regex]::Match($line, 'painted=(\d+)').Groups[1].Value
    $totals += [int]$total
    $phases += [pscustomobject]@{ Dom = [int]$dom; Painted = [int]$painted }

    Write-Output ("run {0,2}: {1,4} ms total   dom {2,3} ms   painted {3,3} ms   (webview start {4,3} ms)" -f
        $i, $total, $dom, $painted, ([int]$total - [int]$painted))
}

Get-Process launch-deck -ErrorAction SilentlyContinue | ForEach-Object {
    $_.Kill(); $_.WaitForExit(6000) | Out-Null
}

Write-Output ""
if ($totals.Count -eq 0) { Write-Output "FAIL  no runs produced a mark"; exit 1 }

$mean = [int](($totals | Measure-Object -Average).Average)
$best = ($totals | Measure-Object -Minimum).Minimum
$worst = ($totals | Measure-Object -Maximum).Maximum
$domMean = [int](($phases.Dom | Measure-Object -Average).Average)
$paintMean = [int](($phases.Painted | Measure-Object -Average).Average)

Write-Output "boot (process start -> rows painted): mean $mean ms, best $best ms, worst $worst ms"
Write-Output "measured at $load% CPU load (busiest process: $busiest)"
if ($load -gt 25) {
    Write-Output ""
    Write-Output "  WARNING: this machine was busy. The pre-page phase is WebView2 starting a"
    Write-Output "  browser engine and it scales with contention, so the total above is not"
    Write-Output "  comparable with one taken on an idle machine. The page-side phases below"
    Write-Output "  are far less load-sensitive -- judge frontend changes on those."
}
Write-Output ""
# The split matters more than the total. Most of boot is WebView2 starting a
# browser engine, which no amount of frontend work touches -- so a bundle change
# should be judged on the page-side phases, not on the headline number.
Write-Output "  before the page exists (WebView2 + Rust setup)  ~$($mean - $paintMean) ms"
Write-Output "  page start -> DOM interactive                   ~$domMean ms"
Write-Output "  DOM interactive -> rows painted                 ~$($paintMean - $domMean) ms"
