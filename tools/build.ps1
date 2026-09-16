# The one correct way to build a release binary of Launch Deck.
#
# It exists because two separate traps sit on this path, and each has now cost
# time twice:
#
#   1. `cargo build --release` produces a binary that still points at the Vite
#      dev server. It compiles, it links, it runs, and it shows "localhost
#      refused to connect" -- because only the Tauri CLI builds the frontend and
#      hands the assets to tauri-build for embedding.
#
#   2. A running Launch Deck holds its own .exe open, so the link step fails
#      with "Access is denied. (os error 5)" -- six minutes of compilation
#      thrown away at the last step, and the error names neither the file nor
#      the reason.
#
#   tools\build.ps1            release binary, no installer
#   tools\build.ps1 -Bundle    release binary + NSIS installer

param(
    [switch]$Bundle
)

$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

$running = @(Get-Process launch-deck -ErrorAction SilentlyContinue)
if ($running.Count -gt 0) {
    Write-Output "closing $($running.Count) running instance(s) so the linker can write the exe"
    $running | ForEach-Object { $_.Kill(); $_.WaitForExit(8000) | Out-Null }
    Start-Sleep -Milliseconds 600
}

# Not $args -- that is a PowerShell automatic variable and assigning to it is
# a runtime error with a message that names neither the variable nor the line.
$cli = @("tauri", "build")
if (-not $Bundle) { $cli += "--no-bundle" }

Write-Output "npx $($cli -join ' ')"
& npx @cli
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$exe = "target\release\launch-deck.exe"
Write-Output ""
Write-Output "built $exe  ($([int]((Get-Item $exe).Length / 1MB)) MB)"

# Prove the binary actually carries the bundle that was just built.
#
# `tauri_build` embeds dist/ at compile time, and until build.rs declared dist/
# as an input, cargo had no reason to relink when only the frontend changed --
# so `tauri build` printed success and shipped the PREVIOUS frontend. The fix
# is in build.rs; this is the check that the fix is working, because the
# failure mode is silent and looks like a bug in the code you just changed.
$indexHtml = "dist\index.html"
if (Test-Path $indexHtml) {
    # Kept on one line each: PS 5.1 mis-parses a `-Raw` switch inside a
    # multi-line method-call argument list and reports it as a type-conversion
    # error on a line that contains no conversion.
    # NOT $bundle: PowerShell variable names are case-insensitive, so that is
    # the `[switch]$Bundle` parameter above, and assigning a string to it fails
    # with a type error reported against this line -- which contains no
    # conversion and names neither the parameter nor the collision.
    $html = Get-Content $indexHtml -Raw
    $mainChunk = [regex]::Match($html, 'assets/(index-[A-Za-z0-9_-]+\.js)').Groups[1].Value

    if ($mainChunk) {
        # Latin-1 maps every byte to exactly one char, so a binary file round
        # trips through a string without loss and IndexOf can do the scan in
        # one call instead of a seven-million-iteration PowerShell loop.
        #
        # By codepage, not `[Text.Encoding]::Latin1` -- that property is .NET
        # Core only and is null under PowerShell 5.1, which fails with "you
        # cannot call a method on a null-valued expression".
        $latin1 = [Text.Encoding]::GetEncoding(28591)
        $haystack = $latin1.GetString([IO.File]::ReadAllBytes((Resolve-Path $exe)))
        if ($haystack.IndexOf($mainChunk, [StringComparison]::Ordinal) -ge 0) {
            Write-Output "verified: the exe carries $mainChunk"
        } else {
            Write-Output ""
            Write-Output "FAIL  the exe does NOT contain $mainChunk -- it is carrying an older frontend."
            Write-Output "      Nothing you changed in src/ is in this binary. Force a relink:"
            Write-Output "          cargo clean -p launch-deck --release"
            exit 1
        }
    }
}
