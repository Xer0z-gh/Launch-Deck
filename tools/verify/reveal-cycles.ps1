# Reveal must keep working, not just work once.
#
# Close-to-tray turned "hidden" into the app's normal resting state, so
# revealing from the Start menu is now the ordinary way back in -- it has to
# survive being used all day, not once. Two bugs here already worked on the
# first cycle and stopped later: show() called off the window's owning thread,
# and a watcher that ended itself on any non-zero wait result. Neither was
# visible in a single-cycle test. This runs the cycle repeatedly and reports
# WHICH cycle failed, because "fails from five onward" and "fails at random"
# have different causes.
#
#   powershell -ExecutionPolicy Bypass -File tools\verify\reveal-cycles.ps1
#   powershell -ExecutionPolicy Bypass -File tools\verify\reveal-cycles.ps1 -Cycles 20

param(
    [int]$Cycles = 8,
    [string]$Exe = "target\release\launch-deck.exe"
)

$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..\..")

if (-not (Test-Path $Exe)) { Write-Output "FAIL  no release build at $Exe"; exit 1 }

# Deliberately NOT Process.MainWindowHandle. That property enumerates only
# VISIBLE top-level windows, so a hidden-to-tray window reads identically to a
# dead process -- which is exactly the distinction this harness exists to make.
# Enumerating by owning PID finds the window whatever its visibility, and by PID
# rather than title so a title change cannot silently turn this green.
Add-Type @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class Win {
    delegate bool EnumProc(IntPtr h, IntPtr p);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr p);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
    [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h, uint msg, IntPtr w, IntPtr l);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowTextLengthW(IntPtr h);

    public static IntPtr MainWindowOf(uint pid) {
        IntPtr found = IntPtr.Zero;
        EnumWindows(delegate(IntPtr h, IntPtr _) {
            uint owner;
            GetWindowThreadProcessId(h, out owner);
            // A titled top-level window. Tray icons and message-only windows
            // belong to the same process and have no caption, so requiring one
            // picks out the real window without depending on what it says.
            if (owner == pid && GetWindowTextLengthW(h) > 0) { found = h; return false; }
            return true;
        }, IntPtr.Zero);
        return found;
    }
}
"@

function Get-Deck {
    Get-Process launch-deck -ErrorAction SilentlyContinue | Select-Object -First 1
}

function Get-DeckWindow {
    $p = Get-Deck
    if (-not $p) { return [IntPtr]::Zero }
    return [Win]::MainWindowOf([uint32]$p.Id)
}

function Test-Shown {
    $h = Get-DeckWindow
    if ($h -eq [IntPtr]::Zero) { return $false }
    return ([Win]::IsWindowVisible($h) -and -not [Win]::IsIconic($h))
}

function Wait-Shown([int]$TimeoutMs) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt $TimeoutMs) {
        if (Test-Shown) { return [int]$sw.ElapsedMilliseconds }
        Start-Sleep -Milliseconds 15
    }
    return -1
}

Get-Process launch-deck -ErrorAction SilentlyContinue | ForEach-Object {
    $_.Kill(); $_.WaitForExit(6000) | Out-Null
}
Start-Sleep -Milliseconds 400

Start-Process -FilePath $Exe
if ((Wait-Shown 30000) -lt 0) { Write-Output "FAIL  app never showed a window on first start"; exit 1 }
Start-Sleep -Milliseconds 800
Write-Output "app started, window visible"
Write-Output ""

$failures = @()
$times = @()

foreach ($i in 1..$Cycles) {
    # WM_CLOSE posted straight at the window is the real gesture -- it is what
    # the titlebar X sends -- so this exercises the actual close-to-tray path.
    # Process.CloseMainWindow() cannot be used for the same reason
    # MainWindowHandle cannot: it looks the window up by visibility, so it
    # silently does nothing on the hidden window this test needs to poke.
    $h = Get-DeckWindow
    if ($h -eq [IntPtr]::Zero) { $failures += $i; Write-Output ("cycle {0,2}  FAIL  no window before hide" -f $i); continue }
    [Win]::PostMessageW($h, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null

    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt 5000 -and (Test-Shown)) { Start-Sleep -Milliseconds 15 }
    if (Test-Shown) { $failures += $i; Write-Output ("cycle {0,2}  FAIL  window never hid" -f $i); continue }

    Start-Sleep -Milliseconds 300

    $t0 = [Diagnostics.Stopwatch]::StartNew()
    Start-Process -FilePath $Exe
    $shown = Wait-Shown 8000
    $t0.Stop()

    $n = (Get-Process launch-deck -ErrorAction SilentlyContinue | Measure-Object).Count

    if ($shown -lt 0) {
        $failures += $i
        Write-Output ("cycle {0,2}  FAIL  no reveal within 8000 ms   instances {1}" -f $i, $n)
    } else {
        $times += $t0.ElapsedMilliseconds
        Write-Output ("cycle {0,2}  ok    {1,4} ms                    instances {2}" -f $i, $t0.ElapsedMilliseconds, $n)
    }
    Start-Sleep -Milliseconds 400
}

Write-Output ""

# Duplicates are the failure this whole mechanism exists to prevent, so they are
# checked even when every reveal succeeded -- a fast reveal that also leaks a
# process is not a pass.
Start-Sleep -Seconds 2
$settled = (Get-Process launch-deck -ErrorAction SilentlyContinue | Measure-Object).Count
if ($settled -eq 1) {
    Write-Output "PASS  exactly one instance after settling"
} else {
    Write-Output "FAIL  $settled instances after settling - expected 1"
}

if ($times.Count -gt 0) {
    $mean = [int](($times | Measure-Object -Average).Average)
    $best = ($times | Measure-Object -Minimum).Minimum
    Write-Output "reveal mean ${mean} ms, best ${best} ms, over $($times.Count) successful cycles"
}

Get-Process launch-deck -ErrorAction SilentlyContinue | ForEach-Object {
    $_.Kill(); $_.WaitForExit(6000) | Out-Null
}

if ($failures.Count -eq 0 -and $settled -eq 1) {
    Write-Output "PASS  $Cycles of $Cycles reveals"
    exit 0
}
Write-Output ("FAIL  {0} of {1} reveals failed - cycles {2}" -f $failures.Count, $Cycles, ($failures -join ", "))
exit 1
