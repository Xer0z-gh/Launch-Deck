# LAUNCH DECK — BUILD LOG

The narrative record of how each slice was built, in the order it happened.
Superseded as the planning document by ROADMAP.md (Vesper-style master plan,
2026-08-01); kept because the reasoning behind each fix lives here and nowhere
else.

Ordered by dependency, not by appeal. Each slice is fully working before
the next begins.

======================================================================
DONE — SLICE 0: THE ENGINE
======================================================================

• Cargo workspace, four crates, dependencies pointing inward
• `deck-domain` — types, traits, run-state machine        (53 tests)
• `deck-runners` — detection, 31 manifests, guarded scan  (86 tests)
• `deck-runtime` — job objects, supervisor, log pipeline  (42 tests)
• Design tokens (`src/styles/tokens.css`)
• `docs/ARCHITECTURE.md`, `docs/RUNNERS.md`, `README.md`

Verified: 181 tests, clippy clean at pedantic, real processes spawned
and killed including a grandchild-survival test.

======================================================================
DONE — SLICE 1: MAKE IT RUNNABLE (2026-07-29)
======================================================================

Ends with an installed app: NSIS installer, Start Menu entry, no
terminal anywhere. Add a folder (pick, drop, or scan), see what it is
and the exact command Run will execute, press Run, watch the output
live, stop it cleanly.

WORKSTREAM 1 — `deck-store`                                    [DONE]
    SQLite via SQLx, embedded migrations, WAL. Queries are runtime-bound
    and every repository method is exercised against a real temp
    database (18 tests) — see ARCHITECTURE for why no `.sqlx` cache.
    Schema: projects (detected facts + overrides + organisation), runs,
    settings. Repositories return domain types, never rows.

WORKSTREAM 2 — `src-tauri` IPC                                 [DONE]
    18 commands, all thin adapters. `DeckError` -> `{code, message,
    retryable}`. Log events coalesce on a 60 ms / 400-line flush; state
    events land immediately. Single-instance focuses the existing
    window; app exit terminates every supervised tree. TS types mirror
    the Rust DTOs hand-to-hand in `src/lib/ipc.ts`.

WORKSTREAM 3 — Dashboard shell                                 [DONE]
    Toolbar (search with Ctrl+K/Ctrl+F, view toggle, archived toggle,
    theme cycle), table + card views with the table default past 30
    projects, one contextual primary action per row, overflow menu.
    Loading skeleton, designed empty states (first-run, no-matches,
    error-with-retry), toasts. Arrow-key row navigation, Enter opens
    logs, visible focus everywhere.

WORKSTREAM 4 — Add-project flow                                [DONE]
    Folder picker, whole-window drag-and-drop, and workspace scan with a
    review-and-select list. Detected type shown with alternatives as
    radio options and the exact run command previewed before anything is
    registered; unidentified folders take a typed custom command
    (quote-aware, never shell-parsed). Duplicates surfaced, truncated
    scans labelled.

WORKSTREAM 5 — Log viewer                                      [DONE]
    Slide-over panel: windowed rendering (fixed 20px rows, ~40 live DOM
    nodes for 5,000 lines), ANSI SGR colours (16/256/truecolour), stream
    filters, search, error-line emphasis, copy, export via save dialog,
    follow mode with wheel-up pause and a jump-to-latest bar, dropped-
    line count when the ring overflowed. Run/stop/restart in the header.

======================================================================
DONE -- SLICE 2: MONITORING (2026-07-30)
======================================================================

• `deck-runtime::metrics` samples the whole process TREE at 1 Hz and
  sums it. Verified on a real project: Auto-Control reported 188 MB
  across 6 processes, where the pid we spawned alone would have read as
  a few megabytes at 0%.
• CPU is a percentage of ONE core and is not clamped -- a four-core
  bundle reads ~400%, which is the truth and the interesting case.
• `deck-runtime::ports` reads the TCP table via `GetExtendedTcpTable`
  and attributes listening ports to the tree. Verified: port 7373
  discovered for a live project, plus a test that binds an ephemeral
  port and asserts it is attributed to this process.
• Samples live in a bounded in-memory ring (120 per project, two
  minutes). Nothing is persisted -- the database stores run outcomes,
  not telemetry -- and a project's history is dropped when its run ends,
  so no stale figure survives on screen.
• ONE sampler and ONE coalesced `deck://metrics` event per tick for all
  projects: refreshing the process table is the expensive part, so it
  happens once. Nothing is emitted when nothing is running, so an idle
  Launch Deck is genuinely idle.
• Row list shows CPU/RAM with a hand-rolled SVG sparkline; the detail bar
  adds ports, PID and process count. The sparkline is hand-rolled because
  a charting library rebuilds its SVG tree per update, which is visible
  jank at 1 Hz across every row.
  (Recharts was installed for the future detail pages and never imported;
  removed 2026-07-30 with six other zero-import dependencies. Re-add it
  when Slice 3 actually needs a chart, not before.)

PORT CONFLICTS ARE TWO-TIER, and the distinction is the point:

    required = ports the USER configured -> launch is REFUSED
    likely   = ports the RUNNER guesses  -> warning logged, launch proceeds

`node.toml` lists 3000, 5173 and 8080 because Node projects commonly use
one of them, not because any project uses all three. The first cut gated
on that list, which would have refused to start a project because an
unrelated app held 8080. A guess may warn; only a fact may block.

STILL OPEN from this slice:
• Health probes (HTTP, TCP, log-match). The `health` manifest field is
  parsed and carried but not yet acted on.
• Disk and network throughput are sampled but not surfaced.

======================================================================
SLICE 3: PROJECT DETAIL
======================================================================

Overview · Logs · Performance · Settings · Environment · Dependencies ·
Build history · Crash history · Notes.

Environment values masked by default and scrubbed from logs. `.env`
files read, never written.

======================================================================
SLICE 4: ORGANISATION AND SEARCH
======================================================================

• Command palette (Ctrl+K) — the primary interaction at scale
• Global search across name, language, framework, tags, description,
  path, category
• Collections, categories, tags, favourites, pinned, archived
• Sorting and filtering that persist across launches

======================================================================
SLICE 5: RESIDENT BEHAVIOUR
======================================================================

• Tray icon with running count and per-project quick actions
• Start-with-Windows
• Window state persistence
• Open in IDE / open folder / open terminal at the project directory
• Bulk actions: stop all, start a collection

======================================================================
DONE — UI HARDENING PASS (2026-07-30)
======================================================================

Triggered by a real bug: with the log panel open, the project list's
columns rendered on top of each other and the action buttons left the
visible area.

Root cause: viewport breakpoints on a container-width problem. The
window was 1240px while the list had ~470px, so `lg:` variants fired for
a container that could not fit half its columns. Fixed by moving every
breakpoint in the list to a CONTAINER query, and by deleting the
`<colgroup>` whose widths had to be kept in lockstep with which cells
were hidden — two lists that drift apart silently. Width and visibility
now live on the same `<th>`.

Also: at narrow widths the name column was being squeezed to ~38px. The
row now sheds its secondary actions (Restart, Logs) into the overflow
menu below a 480px container and drops the word "Status" to just its
dot, so the name keeps a readable width. Both shed actions gained menu
entries — a control that is hidden at some window sizes and absent from
the menu is a feature that silently does not exist.

Guideline pass against the Vercel Web Interface Guidelines fixed:
`color-scheme` tracking `data-theme` (native scrollbars were rendering
light on a charcoal panel), `theme-color`, `overscroll-behavior:contain`
on dialog bodies, `content-visibility:auto` on rows so a large library
does not lay out off-screen work, `autocomplete`/`spellcheck` off on
text inputs, `text-wrap: balance` on headings, `touch-action` on
controls, and a real focus indicator on table rows — an inset ring,
because the previous background-change was invisible on a selected row,
which already used that background.

`docs/DESIGN.md` written: the design rules with the reasoning behind
each, plus the conflict order (correctness > legibility > reachability >
density > elegance) so the next such argument is short.

Verified: 258 Rust tests, tsc and ESLint clean, 34/34 layout geometry
checks at 1600/1240/1024/900 with the log panel open and closed, 28/28
UI checks, 16/16 guideline checks read from computed styles in the
shipped release build.

======================================================================
DONE - CHARACTER, MOTION AND WEIGHT (2026-07-30)
======================================================================

Tanner: the UI "looks bland". Diagnosis: at rest the app was entirely
greyscale, because the accent is reserved for running processes and
usually nothing is running. Twenty-seven rows repeated the word "Idle"
in identical grey, and near-identical glyphs gave no way to tell a Rust
project from a Python one without reading.

Fixes, in order of how much they mattered:

* Language identity colours tinting each project's icon tile. Muted on
  purpose - saturation is what separates identity from status, so a
  tinted tile labels while a vivid dot reports.
* "Idle" recedes to ink/30; the states that matter step forward.
* A 2px accent edge on the leading side of a running row, so "live"
  survives peripheral vision in a long list.
* Surfaces given a faint cool cast. They were mathematically neutral
  (R=G=B), which is the most default a palette can be. Also a 1px
  hairline light-catch on raised surfaces.
* The table header now sits on the field colour, so the table has a lid.

Motion via motion.dev: log panel (animating WIDTH, not x-position, so
the list reflows with it rather than appearing to be shoved aside),
detail bar (height), and the toast stack. `LazyMotion` + `domAnimation`
with `strict`, so the full library cannot be pulled back in by accident.

Also removed seven dependencies with zero imports - recharts, cmdk,
three unused Radix packages, and the Tauri opener plugin, which was
registered on the Rust side but never called from either side (all three
open commands use `spawn_detached` directly).

STYLE POLICY CORRECTED. This document previously carried a list of
banned styles. Tanner corrected that on 2026-07-30: he never asked for
one, and had it recorded by mistake. Every style is available; the flat
instrument register here is a choice justified by *this* product's
purpose, not a rule. Only two constraints are actually his: SVG icons
never emoji, and typography craft. See DESIGN.md section 9.

Verified: 258 Rust tests, clippy clean at pedantic, tsc and ESLint
clean, 27/27 UI, 34/34 layout, 16/16 guideline checks, and the panel
animation caught mid-flight at 217px settling to 496px. Bundle
426 -> 509KB (motion's real cost, lazy-loaded).

======================================================================
FIXED - THE INVISIBLE GITIGNORE BUG (2026-07-30)
======================================================================

Symptom: on a wide window with the log panel open and full of long
lines, the panel grew unbounded, squeezing the project list to ~240px.
Columns overlapped, the name column vanished, the detail bar stacked on
itself. Exactly the failure the container-query work was supposed to
have ended.

Cause, and it is worth remembering: `D:\Workspace\.gitignore` line 36
contains `logs/`, a rule meant for log OUTPUT directories. It also
matches `src/features/logs/` -- the log viewer's SOURCE.

    git check-ignore -v src/features/logs/LogPanel.tsx
    .gitignore:36:logs/    src/features/logs/LogPanel.tsx

Two silent consequences:

1. **Tailwind v4 auto-detects sources but skips gitignored paths.** Every
   utility used ONLY by the log panel produced no CSS whatsoever. That
   is why the panel had `max-width: none` in the browser while
   `max-w-[560px]` sat plainly in the source. Classes it shared with
   other files (`text-[13px]`) still worked, which is what made this so
   well hidden -- the panel looked 95% correct.
2. **The entire directory was never committed.** `git ls-files
   src/features/logs/` returned nothing.

The tell was that a brand-new probe class added to that file also failed
to generate, which proved the file was not being read rather than the
utility being unsupported.

Fixes:
* `.gitignore` here re-includes `!src/features/logs/`.
* `tokens.css` declares `@source "../**/*.{ts,tsx}"` so utility
  generation no longer depends on ignore rules from a parent repo.
* Panel clamp raised to 880px -- 560 was cramped for real log lines on a
  1920 display.

Verified with a project actually running and producing long output at
1920: panel 768px (capped 880), scrollWidth 767 so content no longer
expands it, list 902px, zero column overlaps, name column 336px.

======================================================================
DONE - NEAR-BLACK, ONE COLOUR STORY (2026-07-30, third pass)
======================================================================

Tanner, on the previous pass: "i dont like the scattered colors ... make
it like a darker gray or like lighter black and give it more colors."

Both halves of that are consistent once you separate WHERE colour goes
from HOW MUCH there is. The per-language icon tints were removed - twelve
hues down a list is confetti, not a system, and it put colour on the one
surface that should stay quiet. Colour moved into the chrome, where it
tells one story, and there is now MORE of it than before:

    brand mark | primary action | active collection | anything live

The surface ladder dropped to near-black (field #101215) so that colour
can actually glow on it, with the steps spaced further apart so a row, a
hovered row and a menu are distinguishable at a glance.

A blue->violet gradient (`--gradient-accent`) is the brand colour.
`--color-accent-2` exists only as its far end and is never used flat, so
"one accent, one meaning" survives. Hover and selection are accent-tinted
rather than another step of grey: on near-black, grey-on-grey hover is
nearly invisible.

Motion: rows arrive in a capped cascade, done in CSS rather than a motion
component per row - Vantage already proved per-row motion components are
the expensive part, and an entrance needs no interruption, gesture or
exit. The sidebar's active item slides, the primary button's glow lifts
on hover (the glow moves, not the button, so a control row never shifts
under the cursor).

The harness's cast<=8 rule caught the palette drifting blue as surfaces
lightened - twice. Both times the palette was fixed, not the threshold.

Verified: 27/27 UI, 34/34 layout, 16/16 guideline checks, stable across
two consecutive passes.

======================================================================
DONE - COLUMN SORTING AND THE "ADDED" STAT (2026-07-30)
======================================================================

Task-Manager behaviour: click a header to sort, click again to reverse,
click a third time to clear back to the default order. That third state
is not decoration -- without it there is no way back to "pinned first"
once you have sorted, short of guessing which column was the default.

Sortable: Status, Name, Kind, CPU, Uptime, Last run, Added. Actions is
not sortable; it holds controls, not a value.

New column: **Added** (`createdAt`) - when the project was registered
here, NOT when the folder was created on disk, which Launch Deck does
not know. It joins the existing "Last run" (`lastLaunchedAt`), which was
already present but only appears past a 1000px container, so it was
easy to miss with the log panel open.

Three decisions worth keeping:

* **An explicit sort is obeyed literally.** The default order floats
  pinned and favourite projects to the top; an explicit sort does not.
  A row out of alphabetical order because it happens to be pinned reads
  as a bug to the eye that asked for alphabetical.
* **Missing values sort last in BOTH directions.** Ascending should not
  open with a wall of "never". Absent is not a small value, it is the
  absence of one.
* **Status sorts by attention, not alphabet** - running first, idle
  last. Alphabetical would open with "Crashed" and bury "Running".

Ties fall back to name so the order is total; without that, equal rows
shuffle between renders and look broken even though the sort is correct.

PERFORMANCE: sorting by CPU needs the metric samples, which arrive every
second. Subscribing the app shell to those unconditionally would
re-render the whole tree once a second forever, so the selector returns
a stable empty object unless the CPU sort is actually active.

The sort is persisted (`deck.sort.v1`), so it survives a restart.

Found while verifying: the UA stylesheet sets `text-transform: none` on
<button>, which Tailwind's preflight does not override -- so the new
button-based headers rendered in sentence case while the one remaining
non-button header stayed uppercase. Restated explicitly on the button.

Verified: 15/15 sort checks driving real mouse clicks against the
shipped build, asserting the row ORDER matches an independently sorted
copy of the same names -- not merely that a caret appeared. Plus 27/27
UI, 34/34 layout, 16/16 guideline, 258 Rust tests.

======================================================================
DONE - ICON, AND THE LAST OF THE BLEND (2026-07-30)
======================================================================

New app icon, built to the construction measured off Chess Scout (the
reference Tanner named): tile inset ~1.5%, corner radius ~19%, white
silhouette filling ~73%, flat fill, no outline, transparent outside the
rounded rect. The old icon was a thin triangle on TRANSPARENCY with no
tile at all, which is why it looked weak on a dark taskbar and nearly
vanished at 16px.

The mark is a thick upward chevron over a narrower pad. The obvious
choice -- a solid triangle above a solid bar -- is the universal EJECT
glyph and reads as "eject" before anything else. A rocket was drawn and
rejected: its fins merge into a blob by 16px. Variants were rendered at
16/24/32/48 and compared before choosing.

The in-app brand mark was updated to the same shape, so the title bar
and the taskbar agree about what this product's mark is.

Also removed the list's outer border, finishing the blend from the
previous pass: with rows already on the field colour, that border was
the only thing still drawing a box around the centre.

BUILD GOTCHA, and it cost three rebuilds to find:

  `tauri-build` emits `cargo:rerun-if-changed` for tauri.conf.json and
  `capabilities` ONLY -- never for the icon files. Replacing
  `icons/icon.ico` therefore does NOT invalidate the build script, and
  the cached compiled Windows resource keeps embedding the OLD icon.
  `cargo clean -p launch-deck` did not fix it either.

  The fix is to touch `tauri.conf.json`.

  Do not trust `ExtractAssociatedIcon` to tell you whether it worked --
  it goes through the shell icon cache. Verify by searching the exe for
  the icon frames' bytes:

      parse the ICO directory, take each frame's first ~48 bytes,
      and assert exe.find(those_bytes) >= 0   -> 7/7 embedded

Verified: 15/15 sort, 27/27 UI, 34/34 layout, 16/16 guideline checks,
plus 7/7 icon frames confirmed present in the shipped binary.

======================================================================
DONE - DEVELOPER DIAGNOSTICS (2026-07-30)
======================================================================

A Diagnostics destination in the sidebar (System > Diagnostics), backed
by a `diagnostics` IPC command.

IT JUDGES, IT DOES NOT DUMP. A screen that prints forty facts makes the
reader do the diagnosis, so its usefulness is capped by their memory of
what each value should be. Every fact the app can evaluate becomes a
CHECK with a severity, and the screen leads with the failures. Raw facts
sit underneath for whatever the checks do not cover.

Checks: database integrity, crash-safe write settings (wal / FULL /
autocheckpoint<=16), WAL-larger-than-database, foreign keys, recovery
snapshot present, runner manifests parsed, runners present, log pumps vs
live processes, log directory size, data directory writable (probed by
actually writing a file, not by reading a permission bit), toolchain on
PATH. Sections: Application, Database, Snapshots, Runners, Runtime, Log
storage, Toolchain -- 44 fact rows.

"Copy report" flattens the whole thing to text, because the point of a
diagnostic is usually to hand it to someone else.

Nothing here mutates state, kills a process or repairs anything: a
diagnostic that also fixes things cannot be run safely when you do not
yet know what is wrong, which is exactly when you want to run it.

The database probe reads its values back from the LIVE connection rather
than repeating the constants in `store.rs`. That distinction is the
whole point -- a report that echoed its own configuration could not have
caught the setting that lost a registry.

IT FOUND A REAL BUG ON ITS FIRST RUN: `PRAGMA integrity_check` reported
corrupt indexes on `runs` (`row 1 missing from index
idx_runs_project_started`). Confirmed from an independent sqlite3
connection, repaired with REINDEX after taking a backup.

    Worth recording: the corrupt index made `SELECT COUNT(*) FROM runs`
    return 8 while a forced table scan (`NOT INDEXED`) returned 3. After
    REINDEX both agree at 3. NO ROWS WERE LOST -- the 8 was the lie. The
    same three run ids and timestamps are present before and after.

Verified: 260 Rust tests (2 new in deck-store), clippy clean at
pedantic, tsc/ESLint clean, 15/15 sort, 27/27 UI, 34/34 layout, 16/16
guideline checks.

======================================================================
DONE - PERFORMANCE, MEASURED (2026-07-31)
======================================================================

Tanner asked for faster launches and faster boot. The first job was to
find out where the time actually goes, because "optimise it" without a
number is just rearranging code.

Instrumentation is now permanent and visible in Diagnostics: startup
phase timings, and a per-launch breakdown of the last 20 launches.

WHAT THE NUMBERS SAID (1920x1080, 20 logical CPUs):

    boot, process start -> rows painted   ~930 ms
      of which runner manifests parsed       0.7 ms
      of which database opened              10-15 ms
      of which ALL of our setup             10-15 ms
      of which Tauri + WebView2 init       ~340 ms
      of which WebView2 page load + React  ~580 ms

    launch, click -> child spawned         43 ms mean
      lookup                                0.2 ms
      plan / detection                      0.2 ms
      port conflict check                   0.2 ms
      spawn                                 42 ms

So the app's OWN backend contributes 10-15 ms of a 930 ms boot. The rest
is WebView2 starting a browser engine, which is not ours to remove.
Claiming a big boot win here would be dishonest.

WHAT WAS ACTUALLY CHANGED:

* The rotating snapshot left `Store::open`. `VACUUM INTO` copies the
  whole database, and running it inside `open` meant the window could
  not appear until the copy finished. It now runs on a background task
  after setup -- same snapshot per launch, no longer in front of the
  user. (Measured effect on boot: none, because the database was never
  the bottleneck. Kept anyway: it is correct, and it stops being free
  the moment the registry grows.)
* The launch path read the whole TCP table TWICE per launch, once for
  required ports and once for likely ports. Now one snapshot serves both,
  and projects declaring no ports skip the syscall entirely.
* Diagnostics is code-split, so its chunk is not parsed on every boot.

Launch mean moved 63 ms -> 43 ms across the session.

THE REMAINING 42 ms IS ONE CALL, and it is now measured rather than
guessed. `resume_main_thread` locates the child's main thread with
`CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD)`, which enumerates every
thread on the machine:

    job.rs test `snapshot_cost`: 33.3 ms mean over 5,923 threads

That exists because `std::process::Child` does not expose the thread
handle `CreateProcessW` already returned, and we spawn suspended so the
job object can be attached before the child can make grandchildren.

The fix is to call `CreateProcessW` directly -- either keeping the
returned `hThread`, or better, passing `PROC_THREAD_ATTRIBUTE_JOB_LIST`
so the process is created inside the job and never needs suspending at
all. That removes ~33 ms of the 43 ms.

NOT DONE, deliberately: it replaces `std::process::Command` on the one
path that guarantees no orphaned processes, and `Command` handles
argument quoting, environment blocks and stdio inheritance correctly.
Worth doing with a clear head and its own test pass, not at the end of a
long session. Cost of waiting: 33 ms per launch, imperceptible to a
human.

======================================================================
DONE - POLISH PASS (2026-07-31)
======================================================================

* App icon is now a flat white mark on transparency, no tile, from an
  SVG master. Tanner: "i want all future app icons to be styled like
  this one" -- recorded as the standard in DESIGN.md section 2b, in the
  vault, and in auto-memory. Supersedes the coloured-tile recipe; Chess
  Scout keeps its tile, new work goes tile-less.
* The loading skeleton now matches the view it precedes. It drew cards
  unconditionally, so opening in table view showed a grid of tiles that
  was then replaced by rows -- the layout visibly changed shape as data
  landed, which reads as a glitch rather than as loading. A skeleton
  that is not the shape of its content is worse than none: it makes a
  promise the render breaks.
* The card view was still on the pre-blend surfaces, so switching views
  changed the apparent brightness of the whole content area. Cards now
  use the same three-step ladder as rows (field resting, panel hover,
  raised selected) and the status dot's ring matches what it sits on.
* Separators use a new `--color-rule` (#2f2f2f). A divider INSIDE a
  surface is not the same object as the groove BETWEEN two surfaces;
  using the dark seam colour for both made every row boundary read as a
  crack in the panel.
* Favourite is a one-click star in the row actions -- the one action you
  perform while browsing rather than as a deliberate operation.
* All four Stop buttons are red, not amber. Amber is reserved for the
  transitional states (stopping/restarting) in the status column.

Verified: 262 Rust tests, clippy clean at pedantic, tsc/ESLint clean,
15/15 sort, 27/27 UI, 34/34 layout, 16/16 guideline checks.

======================================================================
DONE - ONE LINE COLOUR (2026-07-31)
======================================================================

Tanner: the separators in the 3-dot menu were still black, and he wanted
every separator in the app to match the light grey ones.

There were two line colours: a near-black `edge` (#0a0a0a) for "grooves
between panels" and a lighter `rule` (#2f2f2f) for dividers inside them.
The distinction was defensible in theory -- a groove darker than both
sides reads as a physical gap -- and invisible in practice. On a #121212
field a #0a0a0a line reads as a crack, not a groove, and having two line
colours meant the menu separators, dialog rules, panel edges and row
dividers never matched each other.

All 23 of them now use `rule`. `--color-edge` had no remaining
references and was deleted rather than left as a dead token.
`--color-edge-strong` survives for CONTROL borders (the outline button,
the scrollbar thumb) -- objects with an edge, not separators.

The simpler rule is also the easier one to keep: **if it is a line, it
is `rule`.**

Verified by surveying every border the running app paints, not by
reading the source: exactly two values remain, `rgb(47,47,47)` at full
strength for structural rules and the same colour at 70% where lines
repeat densely (row and list dividers). No black border anywhere.

The `ui-verify` assertion was INVERTED to match, and deliberately so: it
used to check "seams are DARKER than the panels they separate", which
after this change would have been testing a token nothing uses. It now
asserts separators are LIGHTER than the surfaces they divide.

======================================================================
KNOWN DEBTS AND OPEN QUESTIONS
======================================================================

• UNEXPLAINED: the app process has vanished twice during harness runs.
  It exits CLEANLY -- no WER crash dump, no Application event-log entry --
  so something is asking it to quit rather than it faulting. Both times
  followed a harness that threw mid-run. The leading hypothesis (abrupt
  CDP disconnect with a stale `Emulation` override) was TESTED AND
  DISPROVED: the app survives that. Cause still unknown. Next step is to
  run it from a console with stderr captured, and log `RunEvent::Exit`
  with a reason so the exit path identifies itself.

• Windows-only today. `deck-runtime::job` is the sole platform-specific
  module; a POSIX implementation would use process groups and
  `SIGTERM`/`SIGKILL` behind the same interface. Nothing above that
  module would change.
• Graceful stop relies on children exiting on stdin EOF. Tools that
  ignore EOF get terminated after the grace period. There is no better
  option from a windowed process on Windows — documented in
  `docs/ARCHITECTURE.md`, not hidden.
• `cmake.toml`'s `run` target assumes a `run` target exists in the
  CMake project. Users override it in project settings. Acceptable
  because CMake genuinely does not define how to launch its output.
• Log files are pruned per project, but total disk usage across all
  projects is not yet capped.
• The `health` probe field is parsed and carried but not yet acted on.
• There is no UI yet for configuring a project's ports, so the hard
  port-conflict gate (`required_ports`) cannot be reached in practice
  until Slice 3 adds project settings. The warning path is live.
• `kill_on_app_exit = false` is stored but not honourable yet: children
  live in kill-on-close job objects, so they die with the app
  regardless. Honouring it means opting a project out of the job at
  spawn time — decide whether that trade is ever worth it.
• The scan command walks the filesystem on the IPC thread pool; a very
  large scan should move to `spawn_blocking` with progress events.
• Detection re-runs are not scheduled. A project that gains a framework
  keeps its old identity until re-detected manually.
• `metrics-verify.mjs` drives the app through `window.__TAURI__`, which
  release builds deliberately do not expose (ui-verify asserts its
  absence as a security property). That harness therefore only runs
  against a dev build. Either port it to a Rust integration test or
  accept it as dev-only — do not "fix" it by exposing the global.
• Rows use `content-visibility: auto` rather than a windowing library.
  That is enough for fixed-height rows and costs no dependency, but it
  does not virtualize the React tree — every row still mounts. If the
  library reaches thousands of projects, revisit with `virtua`.
