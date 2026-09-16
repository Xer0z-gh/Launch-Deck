# End-to-end proof against the program that actually caused the bug.
#
# Circle-Calculator is an interactive C++ menu. Stopping it closes its stdin,
# which it reads as an invalid menu choice -- so it prints an error, redraws its
# menu, and loops as fast as the pipe allows. Before the fix it did that for the
# full five-second grace period: 4,151,426 lines and 208.7 MB on disk.
#
# Unit tests cover both halves of the fix with synthetic fixtures. This runs the
# real program through the real UI, because the fixture is only ever as good as
# my model of what the real one does -- and my model of this one was wrong until
# I read its log.
#
#   powershell -ExecutionPolicy Bypass -File tools\verify\runaway-cli.ps1

param(
    [string]$ProjectName = "Circle-Calculator",
    [int]$Port = 9280,
    [string]$Exe = "target\release\launch-deck.exe"
)

$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..\..")

$logRoot = Join-Path $env:APPDATA "Launch Deck\logs"
if (-not (Test-Path $logRoot)) { Write-Output "FAIL  no log directory at $logRoot"; exit 1 }

Get-Process launch-deck -ErrorAction SilentlyContinue | ForEach-Object {
    $_.Kill(); $_.WaitForExit(6000) | Out-Null
}
Start-Sleep -Milliseconds 700

$before = Get-ChildItem $logRoot -Recurse -Filter *.log -ErrorAction SilentlyContinue |
          Select-Object -ExpandProperty FullName

$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$Port"
Start-Process -FilePath $Exe
foreach ($i in 1..60) {
    Start-Sleep -Milliseconds 500
    if (Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue) { break }
}
Start-Sleep -Seconds 4

$env:CDP_PORT = "$Port"
$env:DECK_PROJECT = $ProjectName
node "tools\verify\runaway-cli.mjs"
$driveExit = $LASTEXITCODE

Start-Sleep -Seconds 2
$after = Get-ChildItem $logRoot -Recurse -Filter *.log -ErrorAction SilentlyContinue |
         Where-Object { $before -notcontains $_.FullName }

Get-Process launch-deck -ErrorAction SilentlyContinue | ForEach-Object {
    $_.Kill(); $_.WaitForExit(6000) | Out-Null
}

Write-Output ""
if ($driveExit -ne 0) { Write-Output "FAIL  could not drive the UI"; exit 1 }
if (-not $after) { Write-Output "FAIL  no new log file was written"; exit 1 }

$fail = $false
foreach ($f in $after) {
    # Re-stat. `Get-ChildItem` returns FileInfo objects whose Length is a
    # snapshot from when the directory was enumerated, and the child was still
    # writing at that moment -- so reading $f.Length here reported 0 MB for a
    # file that was 19.55 MB on disk, and this harness printed PASS for a run
    # that should have failed its own threshold. A stale measurement that
    # exonerates the thing under test is the worst kind.
    $size = (Get-Item $f.FullName).Length
    $mb = [math]::Round($size / 1MB, 2)
    Write-Output ("new log: {0}  {1} MB" -f $f.Name, $mb)
    # The old behaviour wrote 208.7 MB. The cap is 32 MiB, and a correct run of
    # this program writes a few KB -- so anything even approaching the cap means
    # the flood ran to completion and only the cap stopped it.
    if ($size -gt 5MB) {
        Write-Output ("FAIL  {0} MB written -- the runaway was not cut short" -f $mb)
        $fail = $true
    }
}

$kept = (Get-ChildItem (Split-Path $after[0].FullName) -Filter *.log | Measure-Object).Count
Write-Output "logs kept for this project: $kept"
if ($kept -gt 10) { Write-Output "FAIL  retention kept $kept logs, expected at most 10"; $fail = $true }

if ($fail) { exit 1 }
Write-Output ""
Write-Output "PASS  the runaway is bounded and retention is enforced"
