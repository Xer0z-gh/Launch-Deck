# Asserts that closing the Launch Deck window does not kill supervised projects.
#
# This encodes a bug that shipped: there was no `CloseRequested` handler, so
# closing the window exited the process, `RunEvent::Exit` ran
# `supervisor.shutdown()`, and every project the user had launched was
# terminated. A fixture running as pid 9936 was gone three seconds after the
# window closed.
#
# It is a PowerShell script rather than a CDP harness because the whole point is
# a real Win32 window-close, and because closing the window tears down the CDP
# connection the other harnesses depend on.
#
#   powershell -File tools\verify\survives-close.ps1
#
# Requires: a release build (npx tauri build --no-bundle) and node on PATH.

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$exe = Join-Path $root 'target\release\launch-deck.exe'
$port = 9291
$fails = New-Object System.Collections.ArrayList

function Check($name, $ok, $detail) {
    $tag = if ($ok) { 'PASS' } else { 'FAIL'; }
    Write-Output "$tag  $name$(if ($detail) { "  - $detail" })"
    if (-not $ok) { [void]$fails.Add($name) }
}

if (-not (Test-Path $exe)) { throw "no release build at $exe - run: npx tauri build --no-bundle" }

# A project that stays alive, so "is it still running" has an unambiguous answer.
$fixture = Join-Path $env:TEMP 'launch-deck-close-fixture'
New-Item -ItemType Directory -Force -Path $fixture | Out-Null
@'
import time, os
print(f"alive pid={os.getpid()}", flush=True)
while True:
    time.sleep(1)
'@ | Out-File -FilePath (Join-Path $fixture 'main.py') -Encoding utf8

# Kill, do not CloseMainWindow. Closing the window is now the thing under test:
# it HIDES the app rather than exiting it, so a close-based cleanup leaves the
# old instance alive, holding the single-instance lock and a different debug
# port -- which surfaces as ECONNREFUSED on a port nobody is listening to.
Get-Process launch-deck -ErrorAction SilentlyContinue | ForEach-Object {
    $_.Kill(); $_.WaitForExit(8000) | Out-Null
}
Start-Sleep -Seconds 2

$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$port"
Start-Process -FilePath $exe

$ready = $false
foreach ($i in 1..40) {
    Start-Sleep -Milliseconds 500
    if (Get-NetTCPConnection -LocalPort $port -State Listen -ErrorAction SilentlyContinue) { $ready = $true; break }
}
Check 'app started with a debug port' $ready "port $port"
if (-not $ready) { throw "app never listened on $port" }

# Register and start the fixture through the real IPC surface.
$driver = Join-Path $PSScriptRoot 'close-fixture-driver.mjs'
$out = & node $driver $port $fixture 2>&1

# Match defensively: when the driver fails it prints an empty "PID=", and
# indexing .Matches[1] on no match threw "Cannot index into a null array" --
# an error about PowerShell rather than about what actually went wrong.
$match = $out | Select-String -Pattern 'PID=(\d+)' | Select-Object -First 1
$pid_line = if ($match) { $match.Matches.Groups[1].Value } else { $null }
if (-not $pid_line) { Write-Output "driver output:`n$($out -join "`n")" }
Check 'fixture launched' ($null -ne $pid_line -and $pid_line -ne '') "pid $pid_line"
if (-not $pid_line) { throw 'fixture never started; nothing to assert' }
$childPid = [int]$pid_line

$app = Get-Process launch-deck -ErrorAction SilentlyContinue
$app.CloseMainWindow() | Out-Null
Start-Sleep -Seconds 5
$app.Refresh()

Check 'app survives the window closing' (-not $app.HasExited) "pid $($app.Id)"
$still = Get-CimInstance Win32_Process -Filter "ProcessId=$childPid" -ErrorAction SilentlyContinue
Check 'launched project survives the window closing' ($null -ne $still) "pid $childPid"

# The window must come back, or hiding is worse than the original bug.
Start-Process -FilePath $exe
Start-Sleep -Seconds 6
$procs = @(Get-Process launch-deck -ErrorAction SilentlyContinue)
Check 'relaunch reveals rather than duplicating' ($procs.Count -eq 1) "$($procs.Count) instance(s)"
Check 'window is visible again' (($procs | Where-Object { $_.MainWindowHandle -ne 0 }).Count -eq 1)

# Quitting IS allowed to stop everything - that is the explicit choice, and a
# hard kill is the strongest version of it: kill-on-job-close must still hold.
$procs | ForEach-Object { $_.Kill() }
Start-Sleep -Seconds 3
$orphan = Get-CimInstance Win32_Process -Filter "ProcessId=$childPid" -ErrorAction SilentlyContinue
Check 'kill-on-job-close still prevents orphans' ($null -eq $orphan) 'project torn down with the app'

# Only the fixture is temporary. $driver is a tracked source file now that it
# lives beside this script -- deleting it here would remove it from the repo.
Remove-Item -Recurse -Force $fixture -ErrorAction SilentlyContinue

if ($fails.Count -eq 0) { Write-Output "`nALL CHECKS PASSED"; exit 0 }
Write-Output "`n$($fails.Count) FAILED: $($fails -join ', ')"; exit 1

# Remove the fixture registration.
#
# It used to be left behind, so every later run of `launch-all` reported a
# "broken" project pointing at a temp directory that no longer existed -- noise
# this harness created and then blamed on the app.
try {
    $env:CDP_PORT = "$Port"
    & node "toolserify\close-fixture-driver.mjs" --remove 2>&1 | Out-Null
} catch {}
