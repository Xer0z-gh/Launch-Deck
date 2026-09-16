# Verification harnesses

The browser-side half of the gate in `docs/ROADMAP.md`. Everything here drives
the **shipped release binary** over the Chrome DevTools Protocol — never a dev
server, and never the source.

That distinction is the entire point. This project's most expensive bugs were
all code that was correct and CSS that never reached the page: a `logs/`
gitignore rule silently excluded `src/features/logs/`, so Tailwind generated no
classes for the log panel and it rendered with `max-width: none`. Nothing in the
source review could have caught it. Only measuring the real page could.

## Running them all

```bash
powershell -ExecutionPolicy Bypass -File tools/build.ps1
```

```bash
powershell -ExecutionPolicy Bypass -File tools/verify/run-all.ps1
```

`run-all.ps1` gives **each suite a freshly started app**. They drive real UI —
opening panels, clicking sort headers, emulating reduced motion — and none
restores what it changed, so chained in one process the results are nonsense:
`sort-verify` reported nine failures and `motion-verify` timed out, both of
which pass alone against the same binary.

Always build through `tools/build.ps1`, never `cargo build --release`. That
command leaves the app pointed at the dev server, so it launches to "localhost
refused to connect" and every content check fails against an empty document
while the source is perfectly fine. The script also closes a running instance
first (the app holds its own .exe open, and the linker fails at the last step
after six minutes of compiling) and then **proves the binary contains the
bundle that was just built** — `tauri_build` embeds `dist/` but did not declare
it as a build input, so a frontend-only change silently shipped the previous
frontend.

## Running one

```bash
WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9280" ./target/release/launch-deck.exe
```

```bash
```

`cdp.mjs` is the shared connection. It refuses to run against a dev-pointed
binary and says which command to use, because that failure surfaces as nine
unrelated assertions failing with every measurement reading -1.

## What each one covers

In `run-all.ps1`:

| Harness | Asserts |
|---|---|
| `layout-verify.mjs` | Geometry at 4 widths × log panel open/closed |
| `ui-verify.mjs` | Theme, native-feel and the absence of `window.__TAURI__` in release |
| `guidelines-verify.mjs` | Web interface guidelines, from computed styles |
| `sort-verify.mjs` | Column sorting, driven with real mouse input |
| `motion-verify.mjs` | That transitions **animate** — samples the property every frame and asserts it passed through intermediate values, because a jump cut ends in the right place and passes every static check |
| `logcontent-verify.mjs` | Log pipeline content |
| `a11y-verify.mjs` | Keyboard traversal of the dialogs (focus enters, Tab is trapped, Escape closes and returns focus) and the screen-reader structure of Diagnostics. Drives real keys, because `aria-` attributes in the JSX are not the behaviour |

Run on their own, because each needs its own app lifecycle:

| Harness | Asserts |
|---|---|
| `survives-close.ps1` | Closing the window hides to tray and kills nothing; a hard kill still tears the tree down |
| `reveal-cycles.ps1` | Reveal keeps working across N hide/relaunch cycles, and names the failing cycle |
| `runaway-cli.ps1` | An interactive CLI that floods on stop is cut off and its log is capped — driven against the real program |
| `style-cost.mjs` | No inline style on repeated rows, and the measured cost of adding one. Reloads first — it deliberately adds inline styles to measure them, so a second run against its own leftovers would report them as a regression |

Measurement, not assertion — these report numbers and have no pass/fail:

| Harness | Reports |
|---|---|
| `boot-real.ps1` | Boot with **no debugging port**, plus the machine's CPU load beside the result — CDP roughly doubles WebView2 startup, and load has produced two phantom regressions |
| `launch-profile.mjs` | Click → perceived state and click → backend truth, on the same click |
| `asset-timing.mjs` | Where the page-side milliseconds go, on the original navigation (`--fresh`) and on a reload |
| `render-profile.mjs` | CPU attribution across the boot render |
| `spawnflags` (cargo bin) | Whether our spawn flags cost anything |

Dev-build only:

| Harness | Why |
|---|---|
| `conflict-verify.mjs` | Fixture checked in at `fixtures/port-project`. Drives the app through `window.__TAURI__`, which release builds deliberately do not expose — `ui-verify` asserts its absence on purpose |

## Two rules that keep these honest

**Pick the app window, not the first CDP target.** WebView2 exposes more than
one page target and the extras are blank. Taking whichever came first produced a
full run of failures against an empty document while the real window rendered
correctly beside it — so every harness filters on the target URL.

**A failing assertion is a claim about the app until proven otherwise.** Two of
the failures found here were bugs in the harness (an over-escaped regex, a stale
target), and both were checked against the live DOM before the assertion was
touched. When a deliberate change makes one fail, rewrite it to encode the
*rule* rather than the old value — and say so, because a suite edited to stay
green proves nothing.

**And the harder half of the same rule: a passing assertion is also a claim.**
Every one of these was a real, confident, wrong answer that came from the
instrument rather than the code:

- `Process.MainWindowHandle` enumerates only *visible* windows, so a correctly
  hidden window reads identically to a dead one — reported a reveal bug that
  did not exist.
- `Get-ChildItem` caches `FileInfo.Length`; read while a child was still
  writing it reported **0 MB** for a 19.55 MB file, and printed PASS for a run
  that should have failed its own threshold.
- A running CSS animation forces a style recalc every frame, so a probe that
  starts before the entrance cascade finishes measures animation state — and
  produced a "54% win" from a property the build no longer had.
- CPU load moves work *between* boot phases rather than scaling the total,
  which manufactured a 151 ms improvement that was really ~37 ms.
- Comparing *different files* in each arm of an A/B produced "prewarming is
  worth 4x". Re-run with fresh copies of one file: 6%.

So: compare arms that differ by only the treatment, interleave them, quote the
machine load beside any timing, re-read state after the writer exits — and when
a harness and the product disagree about the product's core mechanism, debug
the harness first.
