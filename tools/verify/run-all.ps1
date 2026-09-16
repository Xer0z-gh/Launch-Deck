# Runs every browser-side harness, each against a freshly started app.
#
# They must not share a process. The suites drive real UI -- opening panels,
# clicking sort headers, emulating reduced motion -- and none of them restores
# what it changed, so running them back to back means each one inherits
# whatever the last left behind. Chained in one process the results were
# nonsense: `sort-verify` reported nine failures and `motion-verify` timed out,
# both of which pass on their own against the same binary.
#
# That is a property of the harnesses, not a bug in the app, but it makes a
# combined run untrustworthy -- which is worse than having no combined run.
# Hence: one app per suite, torn down in between.
#
#   powershell -ExecutionPolicy Bypass -File tools\verify\run-all.ps1

param(
    [int]$Port = 9280,
    [string]$Exe = "target\release\launch-deck.exe"
)

$ErrorActionPreference = "Continue"
Set-Location (Join-Path $PSScriptRoot "..\..")

if (-not (Test-Path $Exe)) {
    Write-Output "FAIL  no release build at $Exe"
    Write-Output "      build with: npx tauri build --no-bundle"
    Write-Output "      (plain ``cargo build --release`` leaves the app pointed at the dev server)"
    exit 1
}

# Release-safe suites only. `conflict-verify` drives the app by calling
# `window.__TAURI__.core.invoke`, which release builds deliberately do not
# expose -- `ui-verify` asserts its *absence* as a native-audit check. So it is
# a dev-build harness by design, and sweeping it in here reports a product
# failure where there is none. Run it against `npm run tauri dev`.
$suites = @(
    "ui-verify", "layout-verify", "motion-verify",
    "guidelines-verify", "sort-verify", "logcontent-verify", "a11y-verify",
    "surface-verify", "prompt-verify", "search-verify",
    "dashboard-verify", "shell-verify", "regress-verify", "launcher-verify"
)

function Stop-Deck {
    Get-Process launch-deck -ErrorAction SilentlyContinue | ForEach-Object {
        $_.Kill(); $_.WaitForExit(6000) | Out-Null
    }
}

$results = @()
foreach ($suite in $suites) {
    Stop-Deck
    Start-Sleep -Milliseconds 600

    $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$Port"
    Start-Process -FilePath $Exe
    foreach ($i in 1..60) {
        Start-Sleep -Milliseconds 500
        if (Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue) { break }
    }
    # Six seconds, not three. Startup now also re-detects every project's
    # runner and rescans the watched folders in the background, and a suite
    # that starts mid-scan competes with it for the filesystem --
    # `guidelines-verify` timed out opening a dialog for exactly that reason
    # while passing 16/16 when run alone.
    Start-Sleep -Seconds 6

    $env:CDP_PORT = "$Port"
    $out = & node "tools\verify\$suite.mjs" 2>&1
    $code = $LASTEXITCODE
    $last = ($out | Select-Object -Last 1)

    $results += [pscustomobject]@{ Suite = $suite; Ok = ($code -eq 0); Summary = $last }
    $mark = if ($code -eq 0) { "PASS" } else { "FAIL" }
    Write-Output ("{0}  {1,-20} {2}" -f $mark, $suite, $last)
    if ($code -ne 0) { $out | Select-String "^FAIL" | ForEach-Object { Write-Output "        $_" } }
}

Stop-Deck
Write-Output ""
$bad = @($results | Where-Object { -not $_.Ok })
if ($bad.Count -eq 0) {
    Write-Output "ALL $($results.Count) SUITES PASSED"
    exit 0
}
Write-Output "$($bad.Count) of $($results.Count) suites FAILED: $(($bad.Suite) -join ', ')"
exit 1
