# Launch Deck — Master Plan

The governing document of the tool every other project is launched from. Launch
Deck is not being "finished"; it is being refined until reaching for anything
else feels slower. Every phase ends with a review against one question: *would
Linear, Stripe or Ableton — with unlimited time — ship this?*

**⚑ 2026-08-31: RE-SCOPE. Launch Deck is the local command center opened every
morning, with exactly three jobs: RUN my apps, DEBUG my apps, and BUILD PROMPTS
for working on my apps with Claude.** Tanner's directive, verbatim rule: *"Any
feature that does not serve one of those three jobs does not ship."* Hard rules
carried with it: no fake data (a tile that is not wired says "not wired" in
plain text), every tile has at least one action, opens in under a second, no
login. The RUN engine stays Rust — Tanner confirmed keeping the supervisor over
the brief's default Node/TS suggestion, by the same logic as his own "do not
rebuild Fleet" rule. Phase 0 audit ran 2026-08-31: 26 of 29 IPC commands live,
nothing stubbed, PROMPT 100% greenfield. Everything below Phase 11 predates the
re-scope; new phases R1-R4 govern from here.

## Phase R0 — Re-scope kills ✅ (Tanner-approved 2026-08-31)
- ❌ **Machine/System view — KILLED.** Wired and real (live PDH/DXGI data) and
  serving none of the three jobs: a Task Manager replica with zero actions,
  525 frontend lines plus a 1 Hz GPU poll. Tanner asked for this screen on
  2026-08-04 and chose to cut it himself under the three-jobs rule — logged
  here so the reversal is a decision, not an accident. Went with it: system.rs
  (~700 lines + 10 tests), `system_stats`, the host warm task, the hostinfo
  example, system-verify.mjs, stats-accuracy.ps1.
- ↩️ **Grid/card view — KILLED, then REOPENED in Phase 17c (2026-09-07).**
  The original reasoning stands as far as it goes: a second presentation with
  strictly fewer capabilities is not worth a view. What it missed is that
  density is itself a capability. Measured in one viewport: 48 tiles fully
  visible against 11 list rows at 1240x820, 20 against 7 at the 900x600
  minimum. That is a different question being answered ("open the one I am
  picturing"), not the same question answered worse, and Tanner asked for it
  directly. The list remains the default and keeps every capability named
  here.
- ❌ **Prewarm byte-read — KILLED by its own measurement.** Fresh-copies A/B
  put it at 6% of a cold launch; the in-app A/B found no difference. The
  PATH-resolution half survives (2.2-4.2 ms per interpreter launch) as a
  resolve-only `prewarm_project`.
- ❌ **Dead-code batch — KILLED.** `project_icon` single binding, `runHistory`
  frontend binding (the backend command and tables STAY — the DEBUG job's
  "last failed run" card is their consumer), `finishedTick`, boot-bench.ps1,
  probe-rows.mjs, the hostinfo example. Relocated out of the repo: icon
  toolchain → `Ops/Icon-Tools`, roadmap template → vault Standards.
- ✅ conflict-verify runs again: its fixture is checked in under
  `tools/verify/fixtures/port-project` instead of a dead session temp path.
- ✅ Gate after the kills: 304 Rust tests, clippy clean, tsc/eslint clean, all
  7 remaining browser suites pass.

## Phase R1 — Surfaces: Fleet + VendSuite wired end to end ✅ (2026-08-31)
A *surface* is what a tile watches that the supervisor does not spawn: a port
someone else opened, a deployed URL, a status file, an external log. Two real
tiles before anything else — Fleet (port 4173 probe, fleet.log tail, open
panel) and VendSuite (HTTPS ping with response time, open site). First network
capability in the app, user-configured URLs only, on demand — never telemetry.

- ✅ `deck_runtime::surface` — port probe (250 ms), HTTP ping via blocking
  ureq on `spawn_blocking` (5 s), external-log mtime. Non-2xx reports its
  real code: a 503 stays 503, never "down" (5 tests, incl. that one).
- ✅ `SurfaceConfig` on `ProjectOverrides` (JSON column — zero migration),
  clearable via double-option patch; all-empty config normalises to `None`.
- ✅ `open_surface_url` refuses anything but http(s) — the scheme gate is in
  the backend, not the UI.
- ✅ Chip in row + detail bar (re-probe on click, 60 s background refresh
  only while the window has focus), Status source… dialog with client-side
  validation, open-site buttons in row actions + detail bar.
- ✅ `surface-verify` (11 checks, in run-all): wires the harness's OWN
  server through the real dialog, asserts the chip's 200 + latency, selects
  the row (a Radix slot bug once unmounted the whole app exactly there),
  kills the server and asserts honest-down, clears and asserts the chip is
  gone. Measured live: Fleet `200 · 2 ms · log 13m`, VendSuite `200 · 192 ms`.

## Phase R2 — Prompt Studio ✅ (2026-08-31)
Context cards as LAUNCHDECK.md per repo, editable in-app; task templates
(debug/feature/refactor/design/copy/ship); one sentence of intent → full
prompt; vault-update footer; hand-off writes NEXT.md (Fleet's own convention);
"Debug this" = card + real error tail + terminal in the repo with `claude`
ready, clipboard fallback.

- ✅ `deck_runtime::studio` — error-line filter (clean log falls back to raw
  tail, never an empty "nothing to see" section), NEXT.md append-merge
  (backlog above always survives — the overwrite would be a data-loss button),
  newest-file + bounded tail reads (6 tests).
- ✅ Commands: `context_card` / `save_context_card` (verbatim markdown, never
  parsed), `error_tail` (live console → last run log → external log, each
  labeled, ANSI stripped), `hand_off`, `open_claude_terminal`.
- ✅ Terminal path: prompt → temp file → new-console PowerShell that sets the
  clipboard FIRST, then runs `claude` (native claude.exe preferred; its argv
  carries multiline prompts intact). Proven live: 30-line prompt on the
  clipboard, claude session opened in the repo. No shell string ever carries
  prompt content.
- ✅ Studio panel: template radiogroup, intent → live byte-for-byte preview
  (the preview IS what ships), card editor in place, Copy / NEXT.md /
  terminal actions; "Debug this…" and "Build prompt…" on every row +
  detail bar.
- ✅ `prompt-verify` (15 checks, in run-all): drives menu → studio → typing →
  card create (asserted on disk) → double hand-off (append proven) → debug
  preset; snapshots and restores the repo files it touches, and asserts the
  restore.
- ⚑ Found in passing: this machine's Windows "Animation effects" toggle is
  OFF, so prefers-reduced-motion is on system-wide — the app correctly
  collapses its transitions. motion-verify now pins no-preference while
  measuring (and still tests reduce explicitly), so the suite measures the
  app, not the OS toggle.

## Phase R3 — remaining tiles ✅ for everything with a folder (2026-08-31)
Wired live, each through the same surface machinery R1 shipped, every reading
verified on this machine the day it landed:

- ✅ **Pavlok-Alerts** — `C:\Users\Computer\.claude\pavlok.status` as the
  watched file (the status file IS the tile). Read live: `log 21h`.
- ✅ **Local-AI (Jarvis)** — NEW row at `Ops\Local-AI`, Run = `ollama serve`,
  pings `127.0.0.1:11434/api/tags` + port probe. Read live: `down` — Ollama
  was off, and the tile said so instead of pretending. Chip mark added in the
  standing icon style.
- ✅ **Lathe** — pings `127.0.0.1:8800` + port probe. Read live: `200 · 2 ms`.
- ✅ **Focus-Forge** — watches `focusforge.log`. Read live: `log 32d`, which
  is itself information: the app has not run in a month.
- ✅ **Finger Skater** — NEW row for the Unity project at
  `D:\Games\Unity Projects\Finger Skater`, Run = Unity 6000.4.11f1 with
  `-projectPath`, watches `Assets\` mtime. Read live: `log 65d`. Fingerboard
  mark added.
- ⏳ **Etsy** — no local folder exists, and the row model is folder-backed;
  an honest Etsy tile needs either a link-tile concept or a decision from
  Tanner. Not faked in the meantime.
- ⏳ **Webflow** — waiting on site URLs from Tanner; nothing measurable
  until then, so nothing shown.

## Review pass on R1-R3 (2026-08-31, same day)

ui-finish-gate + accessibility agents audited the live build: HOLD with
eleven measured defects (layout collision at the real window size, invented
"errors" over a clean log, Esc collapsing layers, no focus management, radio
pills without radio keys, sub-4.5:1 tones, a viewport-overflowing menu).
All fixed in 1b8f724 and suite-enforced (prompt-verify 15 -> 22 checks,
surface-verify 11 -> 14). reality-checker then re-ran every gate itself and
certified PRODUCTION READY, with two open nits carried here honestly:

- ⏳ Contrast is fixed in code but not machine-checked -- extend
  guidelines-verify with computed-ratio assertions on the chip/studio tones.
- ⏳ `open_claude_terminal` has no automated coverage (deliberate: a suite
  popping terminal windows trains the operator to stop running suites). It
  was proven live once on this machine; re-verify by hand after changes.

## Daily-app increment (2026-08-31, Tanner: "my daily app ... at a glance")

- ✅ **Ranked search** (`src/features/projects/search.ts`): name prefix >
  word start > substring > field > path, subsequence fallback ("lchdck"
  finds Launch-Deck), multi-word AND, relevance order while typing, Enter
  takes the top hit. Column sorts resume when the query clears.
- ✅ **Periodic auto-scan**: the startup rescan now also runs every ten
  minutes (local filesystem only -- the network boundary is untouched) and
  toasts "Scan found N new projects" when it adds rows.
- ✅ **Today strip** (R4's first slice): one row above the table -- running
  count, every failure from the last 24 h (`failures_since`, cross-project,
  stops excluded; click = select + open logs), every wired surface that is
  definitely down (shares the chips' query cache -- one probe per project
  per minute, never two). Quiet mornings say "all quiet", because "checked,
  nothing found" and "did not check" must not look alike.
- ✅ **Embedded app windows**: "Open in app window" opens the project's URL
  in a Launch Deck window (per-project singleton, http(s)-gated, no IPC
  capabilities granted to the page). Proven live with Fleet's panel.

## Dashboard + Apple register (2026-08-31, Tanner: "more of an apple style
and a dashboard for everything and all important stats")

- ✅ **Dashboard destination, and the app now lands on it.** Five sections,
  every number measured: headline tiles (running / surfaces down / died 24 h
  / library size, each a control), Live now (real CPU, RAM, uptime per
  running process + Stop), Watched (every wired surface and its reading +
  open-in-app-window), Recent activity (cross-project `recent_runs`, outcome,
  duration, click opens that project's log), Library (language mix, each
  segment filters the list).
- ✅ **Apple register, structure only.** Radii up one step (8/12/16/18), a
  soft wide card shadow, grouped inset cards with hairline dividers, large
  page title, generous section rhythm. The palette did NOT move -- his
  standing rule is that a reference's layout and material travel, its colours
  do not.
- ✅ Three defects the first render exposed, all fixed: log ages rendered as
  calendar dates here but as "65d" in the row chips (one `formatAge` now
  serves both); two runs claimed "running" while the header said nothing was
  running (a history row whose finish was never written now reads "no end
  recorded"); and the page had two `h1`s (outline is now app → page →
  section).
- ✅ `dashboard-verify` (9 checks, suite #11) pins CONSISTENCY: the running
  count must agree across dashboard, table and Today strip; the watched card
  must cover exactly the projects carrying a chip; no activity row may claim
  "running" when nothing is; every tile is a labeled button that navigates.
- ⛑ The ten older suites now navigate to the projects list explicitly
  (`showProjects` in cdp.mjs) instead of assuming it is the landing screen --
  the landing surface is a product decision, and `dashboard-verify` is what
  asserts it.

## Review pass on the dashboard (2026-08-31)

ui-finish-gate returned HOLD, accessibility-auditor five MUST-FIX. Both were
right; everything below is fixed and suite-enforced (dashboard-verify 9 → 13
checks).

- ✅ **The header sentence contradicted its own tiles.** It counted a probe
  that answered WITH a transport error as "answered" (so "7/7 answered" sat
  above a tile reading "1 surfaces down"), and said "nothing died overnight"
  above eight rows reading "no end recorded". Every clause is now built from
  the same values as the tiles, and a suite check pins the agreement.
- ✅ **`aria-label` was replacing tile content.** "Library / 44 projects"
  announced as "44 projects. Show all projects." — the visible word gone
  from the accessible name, every detail line dropped. Names now compose
  from what is rendered; the action phrase is a visually-hidden span and the
  detail rides `aria-describedby`. Same fix for the eight activity rows that
  all announced "Open Auto-Control logs".
- ✅ **A tile with nothing to do is no longer a button** that swallows Enter.
- ✅ **Library SECTION deleted.** It restated the sidebar's language rail
  (same languages, counts and filter action, both on screen at once), and
  its only unique element spent the reserved "live" accent on static data
  and collapsed every language past the fourth into one flat slab. The
  Library stat tile keeps the headline numbers.
- ✅ Recent activity collapses consecutive identical runs (`×17`) instead of
  printing the same row seventeen times.
- ✅ Watched card has one right edge at every width (accessory in a fixed
  slot); interactive rows get a pointer and a visible hover step; name and
  legend targets reach 24px.
- ✅ Light `--color-ink` #1c1c1c → #111111: `ink/60` measured 4.36:1 on the
  light panel, under the floor. Now 4.82:1, and every other light-theme
  opacity improved with it.
- ✅ At the app's 900×600 minimum, the first viewport now holds measured
  values (compact tiles, 4-across from 640px) instead of four 319px boxes.
- ✅ The register is the app's, not one screen's: the projects table now
  carries the same grouped-card material as the dashboard's sections.

### reality-checker returned NEEDS WORK on the first attempt, correctly

Every gate number reproduced, but the headline claim did not: the header
sentence and the tile detail counted the same seven rows two different ways,
and for ~1.2 s on every landing the tile asserted "All 7 watched apps
answering" while one was down. The check written to catch that slept 3.5 s
past the only window in which it happens. Fixed:

- ✅ **One classification, computed once**: every watched surface is exactly
  one of pending / up / down / quiet, and the header, the tile and its detail
  all read it. A verdict is never asserted while probes are unresolved
  ("checking 3 of 7 watched apps"), and a log-only surface is no longer
  counted as "answering" — it reports an age, not an answer, which is what
  `describeReading` always said by toning it dim.
- ✅ **The Died tile waits for its query** instead of rendering 0 before
  `overnight_report` returns.
- ✅ **ROOT CAUSE FIXED, not carried as a caveat**: `RunOutcome::Interrupted`
  plus a startup reconciliation (`reconcile_orphaned_runs`). Nothing is
  spawned during boot, so any row still marked `running` describes a process
  that is not alive — 19 such rows on this machine, growing. They now close
  as `interrupted` with no invented finish time. This removes the data defect
  that regenerated the contradiction: an orphan of a *live* project used to
  render "running ×18" under a header saying one thing was running.
- ✅ **`collapseRuns` stops hiding evidence**: the exit code is part of the
  group identity (exit 1 and exit 137 were collapsing into "exit 1 ×2"), the
  live run is never folded into the orphan group, and a group states the span
  it covers — "interrupted ×19 over 16d", where it used to print only the
  newest timestamp and read as 19 runs a minute ago.
- ✅ **Three tautological checks repaired.** The header-agreement check now
  reads header and tiles in ONE evaluate, is word-anchored (so "41 of 7"
  cannot satisfy a check about 1), fails in both directions, and a new check
  samples the loading window it used to sleep past. Proven by extracting the
  predicates from the suite file and running them against six cases,
  including the two that previously passed for the wrong reason.
- ✅ **`showProjects` returns its result and all ten callers assert it** — a
  suite that navigated, found nothing and carried on would pass every later
  check vacuously; `surface-verify` did exactly that on a missing row.

Known issue, stated with its real consequences:
- ⏳ **White-on-transparency project marks are faint on light surfaces.** The
  icon standard is "flat white mark, no tile", which assumes a dark ground.
  A blanket invert would wreck any genuinely coloured icon, so this needs
  Tanner's call: invert only monochrome marks, or give icons a neutral dark
  plate in light theme. Not a WCAG blocker — every row carries its name at
  full-strength ink — but below the ship bar for a default theme.
- ⏳ **The projects table's `ink/25`–`ink/45` tiers fail AA** in both themes
  (153 of 248 text nodes measured: em dashes 1.7:1, "Idle" 2.0:1, paths
  2.2:1, column headers 2.9:1). Pre-existing, not introduced by the dashboard
  work, and `a11y-verify` does not measure table contrast — so it reports
  21/21 while this stands. Its own pass, with a contrast check added to the
  harness.

## iOS / iPadOS system language (2026-08-31)

Tanner: *"really nail that apple styling fully switch the ui to this"* with the
iOS/iPadOS 27 design kit. Not a corner-radius pass -- the token layer IS
Apple's system now, and the app is built from it.

- ✅ **Palette**: iOS grouped backgrounds (dark: true black page, #1C1C1E
  cards; light: #F2F2F7 page, white cards -- the inverse, which is what makes
  a light theme feel designed rather than dimmed), label steps as Apple's
  tinted-white opacities, the four system fill levels, systemBlue / Red /
  Orange / Green.
- ✅ **Type**: the full iOS ramp with its tracking (Large Title 34 through
  Caption 2 11). SF Pro's METRICS with installed faces only -- `-apple-system`
  picks up SF on a Mac, Segoe UI Variable here. Pulling SF Pro down is exactly
  what Tanner rejected on 2026-08-25.
- ✅ **Shape**: continuous radii 10/14/18/26 plus the capsule, which every
  control now takes -- rounded rectangles read as iOS 15 no matter how correct
  the colour is.
- ✅ **Controls**: Apple's four treatments mapped onto the app's four
  (Filled / Tinted / Plain / Plain-red), press-scale instead of a shade
  change, and text fields seated in a system fill with no border.
- ✅ **Surfaces**: sidebar selection is a filled tint with a white label
  (iPadOS) rather than a raised row with a marker bar; the project list is an
  inset grouped card with 56px rows; grouped cards are flat, because on iOS
  the grouped background does the separating, not elevation.
- ✅ **Kept deliberately**: sortable column headers. A true iOS list has none,
  but iPadOS Files shows exactly this in list view, so the feature survives
  the restyle instead of being deleted as a style casualty.
- ✅ **Known issue CLOSED**: white-on-transparency marks were invisible on the
  light theme's white cards. `useMonochromeIcon` decides per icon, from its
  decoded pixels, whether it is a light monochrome mark; only those invert. A
  coloured icon is left alone rather than turned into a negative.
- ⚑ **A logged decision was reversed, on his instruction.** `ui-verify` used
  to assert the field is never black, because near-black under near-white sits
  near 17:1 and causes strain over an hour. iOS dark mode uses true black and
  calibrates its greys against it. The constraint changed shape rather than
  vanishing: the field may be black, the ladder above it must still ascend,
  and body text must not be pure white -- which is the half that actually
  caused the strain, and which Apple avoids too.

## Phase R4 — The daily hook ⏳
Today strip (what died overnight from run/crash history, yesterday's focus from
context cards), Windows-startup launch, Pavlok buzz when the morning brief is
ready via `pavlok-notify.mjs`.

**⚑ 2026-07-30: the standing quality bar is production software, never a
prototype.** Loading, empty and error states; keyboard navigation and visible
focus; no dead code left behind. Every claim in this document is backed by a
test, a measurement, or a decision someone can argue with. Where a number is
quoted it was measured on this machine, not estimated.

Legend: ✅ shipped · 🔄 in flight · ⏳ planned · 💭 research · ❌ rejected
Complexity: S (<1 day) · M (days) · L (weeks) · XL (a system)

---

## Phase 0 — Vision & identity ✅ (living)
- ✅ Product thesis: a **local-first control centre** for every project on this
  machine. No cloud, no accounts, no telemetry — proven, not asserted (15/15
  native-audit checks; release build exposes no dev hook and no global Tauri API)
- ✅ The core loop is *find one project in a list and press Run*. Everything is
  ranked against that
- ✅ Identity: the chevron-over-pad mark, flat white on transparency
  (docs/DESIGN.md §2b). Rolled out across all seven apps 2026-08-01
- 🔄 Identity audit each phase: does every new pixel still read as an instrument
  rather than a web page?

## Phase 1 — Core architecture ✅ (guarded by tests)
Four crates, dependencies pointing inward: `deck-domain` (types, run-state
machine, zero I/O) → `deck-runners` (detection) → `deck-runtime` (processes,
the only platform-specific layer) → `deck-store` (SQLite) → `src-tauri` (thin
IPC adapters). **262 tests**, clippy clean at pedantic.
- ✅ Run-state machine where `can_start` and `can_stop` are mutually exclusive by
  construction, so "Run" and "Stop" can never both be offered
- ✅ `DeckError` carries `{code, message, retryable}` end to end; the UI retries
  only what is genuinely retryable
- ✅ Runner manifests are **declarative TOML data, not code** — 32 bundled via
  `include_str!`. Adding a language never touches the core
- ❌ **shadcn CLI — REJECTED.** Raw Radix primitives + our own tokens instead.
  Its default look is the most recognisable "AI-generated app" aesthetic, and
  the CLI's copy-in components would have to be restyled anyway
- ❌ **Recharts — REMOVED** (2026-07-30). Installed for future detail pages and
  never imported; removed with six other zero-import dependencies. Re-add when
  Slice 3 needs a chart, not before
- ⏳ [L] A `DECISIONS.md` log. Vesper's `D-0NN` references are the reason its
  roadmap can be audited; this document currently cites tests and measurements
  instead, which works but does not capture *rejected* alternatives as durably

## Phase 2 — Detection & runners ✅
- ✅ 31 manifests: `[meta]` / `[[detect]]` / `[[vars]]` / `[commands]` /
  `[version]` / `[health]`. **Detection never executes anything** — it reads
  files, with a path-traversal refusal and a 256 KiB read cap
- ✅ Workspace scan: 40-entry ignore list, depth cap, stop-on-match; truncated
  scans are labelled as truncated rather than silently short
- ✅ Program resolution honours PATH/PATHEXT and distinguishes `Direct` from
  `ViaShell`. **Every Node project in the library was unlaunchable** until this
  landed: `.cmd` shims are not executable by `CreateProcess`. It survived
  earlier testing only because every prior E2E used a real `.exe`
- ✅ TOML folding trap documented and guarded: `detect = [...]` after `[meta]`
  silently becomes `meta.detect`. The loader rejects empty rule lists with a
  message that names the mistake

## Phase 3 — Supervision & process control ✅
- ✅ Windows **Job Objects** (`KILL_ON_JOB_CLOSE`, `BREAKAWAY_OK`,
  `DIE_ON_UNHANDLED_EXCEPTION`). Verified for real: a test spawns `cmd` → `ping`,
  terminates the job, and asserts the **grandchild** is gone
- ✅ The assignment race is closed by spawning `CREATE_SUSPENDED`, assigning,
  then resuming — otherwise a child can spawn a grandchild that escapes the job
  permanently
- ✅ Graceful stop (stdin EOF) → grace period → `TerminateJobObject`. Documented
  as a platform limit rather than papered over
- ✅ Port conflicts are **two-tier**: `required` (user-configured) refuses the
  launch; `likely` (runner guess) only warns. Gating on guesses would refuse to
  start a project because an unrelated app held 8080
- ✅ The suspend window is closed **and** cheap: the child is resumed through
  `NtResumeProcess` on the handle we already hold, not by enumerating every
  thread on the machine — see Phase 9
- ✅ **Stopping an interactive CLI no longer floods the disk.** Reported as
  "some apps that use a cli get a memory leak and keep entering in the menu",
  and the phrasing was literal. Circle-Calculator is an interactive C++ menu.
  Graceful stop closes the child's stdin, because most CLI tools read EOF as
  "shut down" — this one read it as an invalid menu choice, printed
  `[Error]: Invalid choice!`, redrew its menu, and did that as fast as the pipe
  allowed for the whole five-second grace period while we waited politely.
  **4,151,426 lines and 208.7 MB on disk in five seconds.** Every other log
  this machine has produced is under 4 KB.

  Three separate things had to be wrong at once, and all three are fixed:

  1. **The log FILE was unbounded** while the in-memory ring was not — which is
     precisely why the existing limits hid this. Capped at 32 MiB per run, and
     the cap announces itself in the file it truncates, because a log that just
     stops is indistinguishable from a crash.
  2. **The grace period was a fixed sleep.** It exists to let a program finish
     shutting down; one emitting thousands of lines a second demonstrably is
     not, so waiting out the remainder buys nothing. Output is now sampled
     every 100 ms; two consecutive flooding windows terminate the tree, or one
     window at ten times the threshold, since past that volume the ambiguity a
     second window resolves is already gone and confirming costs megabytes.
  3. **`prune_logs` was called from nowhere but its own tests**, while its doc
     comment said "called after a run finishes". The result was the exact slow
     leak that comment warns about — thirty-odd files accumulated over a week.
     Now called for real, keeping ten runs per project.

  End to end against the real program, via `tools/verify/runaway-cli.ps1` —
  real clicks on the real Run and Stop buttons, because a synthetic fixture is
  only ever as good as my model of the program, and my model of this one was
  wrong until I read its log:

  | | log written | time to stop |
  |---|---|---|
  | before | 208.7 MB | 5 s (the full grace) |
  | two-window rule at 250 ms | 19.55 MB | 146 ms |
  | **shipped** | **3.91 MB** | **146 ms** |

  **53× less disk.** The remaining ~4 MB is one 100 ms detection window against
  a program writing roughly 40 MB/s; taking it lower means a shorter window and
  progressively less certainty about what is a loop and what is a final flush.

  The middle row exists because the harness lied first. It read `Length` from
  the `FileInfo` that `Get-ChildItem` had cached while the child was still
  writing, reported **0 MB**, and printed PASS for a run that should have
  failed its own 5 MB threshold. A stale measurement that exonerates the thing
  under test is the worst kind, and it is the third time today measurement --
  not code — was the thing that was wrong.
- ❌ **Docker Compose runner — REMOVED** (2026-08-04). Tanner's call, after
  Docker Desktop crashed on his machine: *"cut docker out of the picture"*.
  Nothing in the library used it, so nothing broke. The row list is still
  modelled on Docker Desktop's — that is a UI reference, not a dependency.
- ❌ **`CreateProcessW` + `PROC_THREAD_ATTRIBUTE_JOB_LIST` — REJECTED for now.**
  It is the textbook answer and would let the process be born inside the job
  with no suspension at all. But it means giving up `tokio::process` and
  hand-rolling three stdio pipes, the async readers and the exit-status
  plumbing the log pipeline depends on — on the single path that guarantees no
  orphaned processes. The handle-based resume got **97% of the available win
  for a fraction of the risk**, so the remaining gain does not justify it.

  **Now settled by measurement rather than argument** (2026-08-05):
  `cargo run -p deck-runtime --bin spawnflags` times our exact flag combination
  against a plain spawn, and `CREATE_SUSPENDED` is **free** — 3.5-4.6 ms either
  way, on a 13.5 MB binary. There is no remaining gain to justify.

## Phase 3b — Resident behaviour ✅ (closed the crash users actually saw)
**Closing the window used to kill every project you had launched.** There was no
`CloseRequested` handler, so Tauri exited on last-window-close, `RunEvent::Exit`
ran `supervisor.shutdown()`, and the job objects' `KILL_ON_JOB_CLOSE` finished
the job. Reproduced deliberately: a fixture running as pid 9936 was gone three
seconds after the window closed. From the user's seat that is not a window
closing — it is the launcher crashing their apps.

- ✅ **Close hides to the tray; the process, the jobs and the projects survive.**
  The two guarantees in tension were both kept: nothing outlives the supervisor
  (relaxing `KILL_ON_JOB_CLOSE` would reintroduce orphaned dev servers holding
  ports), and closing a window is not a request to stop working.
- ✅ **Tray icon with Show / Stop all / Quit.** Not a nicety: a hidden window
  with no tray icon is strictly *worse* than the original bug, because the app
  becomes unreachable and looks like it crashed.
- ✅ **Tooltip carries the running count**, refreshed on every state change —
  when the window is hidden it is the only surface saying what is still alive.
- ✅ **Single-instance now reveals rather than focuses.** `set_focus` on a
  *hidden* window does nothing, so relaunching from the Start menu would have
  appeared to do nothing at all.
- ✅ **Close-to-tray created a multi-instance bug, now closed.**
  `tauri_plugin_single_instance` stops a duplicate reliably while the window is
  *showing*; once hidden — which close-to-tray made the normal state — a
  relaunch started a whole second instance. **Measured: five processes alive and
  still alive twenty seconds later**, each with its own tray icon, all on one
  SQLite file. That is the contention that cost a 27-project registry once
  already, and this change is what introduced the path, so the fix belongs here.
  A named kernel event now answers "is one already running?" — it has no window
  state to miss. Instances settle to **1** where they used to reach 5.
- ✅ **Reveal holds up under repeated use: 25 of 25 cycles, mean 334 ms.** Two
  real bugs were fixed getting here — `show()` called off the window's owning
  thread, and a watcher that ended itself on any non-zero wait result, which
  quietly disabled the fast path for the rest of the process's life.
- ❌ REJECTED — "reveal degrades after about four cycles". Reported as a live
  bug and it was not one; it was the measuring instrument. The harness asked
  .NET's `Process.MainWindowHandle` whether the window was up, and that property
  enumerates only *visible* top-level windows — so a correctly hidden window
  reads exactly like a dead one, which is the single distinction the test
  existed to make. Rewritten to enumerate by owning PID, the same build passes
  25 of 25. The lesson is cheap to state and was not cheap to learn: when a test
  and the product disagree about the product's core mechanism, suspect the test
  first, because a harness gets far less review than the code it guards.
- ✅ Guarded by `tools/verify/survives-close.ps1`, which also asserts that a
  hard kill still tears the tree down — the fix must not buy reliability by
  giving up the orphan guarantee — and by
  `tools/verify/reveal-cycles.ps1`, which runs the hide/relaunch cycle N times
  and names the failing cycle, because "fails from five onward" and "fails at
  random" have different causes.

## Phase 3c — Setup readiness ✅ (the other half of "it crashes")
**6 of 29 projects in the real registry could not start**, and every one of them
surfaced as a *crash* several seconds after Run. The information was on disk the
whole time: no `node_modules`, or a `requirements.txt` with no virtualenv.

- ✅ **Setup rules live in the runner manifests**, not in Rust — adding an
  ecosystem stays a `.toml` edit, per the standing invariant. Read-only like
  detection: a probe that could execute would turn "open the app" into "run
  whatever 29 folders say".
- ✅ `requires_any` takes a list because the convention genuinely varies —
  `.venv`, `venv`, `env` are all normal, and picking one would report working
  projects as broken.
- ✅ **Run is replaced by Install, not merely annotated.** Leaving Run as the
  primary action invites a launch already known to fail, and the exit code reads
  as "the launcher broke my app".
- ✅ The install command **has been in every manifest from the start** and
  simply had no way to reach a user. It resolves through the manifest's own
  variables, so a pnpm project is offered `pnpm install`, not `npm install` —
  verified against the real registry.
- ⏳ [S] Surface the same state in the detail bar and the grid cards, not only
  the row actions.

## Phase 3d — Launching built apps ✅ (over a minute → 864 ms)
Pressing Run on a Tauri project ran `npm run tauri dev`, which does a full cargo
debug build before anything appears. **Chess Scout took over a minute** while a
finished `chess-scout.exe` sat in `target/release` the whole time.

- ✅ **`tauri-built` runner** (priority 450, above `tauri`) launches the compiled
  binary. It only claims a project when a binary actually exists, so an unbuilt
  project still falls through to the dev-server path — verified: Learning-Center
  correctly stayed on `tauri` because it has no build.
- ✅ **Measured: 864 ms**, of which our own work is **0.2 ms** (lookup 0.1,
  plan 0.1, ports 0.0). The rest is Windows loading a large binary. Confirmed
  the real app came up — window handle, WebView2 and `stockfish.exe` children —
  **with no cargo, rustc or node process anywhere**.
- ✅ **`first_match` reaches into build directories.** Patterns may now name a
  subdirectory (`src-tauri/target/release/*.exe`), and `first_match_any` takes
  an ordered list because a standalone app builds to `src-tauri/target/` while
  one inside a Cargo workspace builds to `target/`. Detection and variable
  resolution share **one** splitter — two matchers that disagree would let a
  runner claim a project it then cannot launch.
- ✅ **`redetect_project`.** Runners are data, but a project's runner was frozen
  at registration, so a new manifest could never reach anything already in the
  library (`rescan_roots` only finds *un*registered projects). Re-detection
  preserves everything the user set — name, tags, favourite, custom command —
  because it corrects OUR guess, not THEIR configuration.
- ⚠️ **The trade, stated:** the binary can be older than the source. A launcher
  launches what is built; `Build` stays a separate lifecycle step and the run
  command names the exact binary, so a stale run is visible rather than
  mysterious.

## Phase 4 — Persistence & durability ✅ (hardened after real data loss)
**27 projects were lost once.** `synchronous=NORMAL` plus the default
`wal_autocheckpoint=1000` left a 45 KB database beside a 362 KB
un-checkpointed WAL. Everything here exists because of that.
- ✅ WAL · `synchronous=FULL` · `wal_autocheckpoint=4` · `foreign_keys=on`,
  and a test that asserts the **file on disk** has them rather than that the
  source asked for them
- ✅ `VACUUM INTO` snapshots, 3 kept, rotated per launch; self-heal restores the
  newest snapshot when the registry is empty. Proven by deleting all 27 real
  rows and relaunching
- ✅ Restore is an **ATTACH + row copy in a transaction**, not a file copy. The
  file-copy version raced Windows handle release and passed only 2 runs in 3
- ✅ Scan roots persist, so the whole library is re-derivable from them
- ❌ **Snapshot inside `Store::open` — REJECTED** (2026-07-31). `VACUUM INTO`
  copies the entire database, so it sat on the critical path of every launch.
  Now a background task after setup: same snapshot per launch, not in front of
  the user. A test asserts `open()` does *not* snapshot, so nobody puts it back
- ✅ **Total log-disk cap, 256 MiB.** Per-project retention keeps ten runs each,
  which is not a bound on the sum of them: this machine's tree reached **90 MB**,
  most of it ten retained copies of one program's runaway output at ~4 MB apiece
  — every file inside every limit that already existed. `prune_logs_total`
  deletes oldest-first at startup, because the run worth reading is almost
  always the last one, and a test asserts the newest survives.

## Phase 5 — Shell & interaction ✅ v1 · 🔄
- ✅ Docker-style row list (default) + card view · sidebar library · search with
  Ctrl+K/F · designed empty states that distinguish "nothing yet" from "nothing
  matched" · toasts · arrow-key row navigation
- ✅ Add flow: folder picker, whole-window drag-and-drop, workspace scan with
  review-and-select. The exact command Run will execute is shown **before**
  anything is registered
- ✅ Log viewer: windowed rendering (~40 live DOM nodes for 5,000 lines), ANSI
  SGR colour, stream filters, search, export, follow-mode with wheel-up pause
- ✅ **Column sorting** (2026-07-30): click sorts, click reverses, third click
  clears back to the default order. Explicit sorts are obeyed *literally* —
  pinned/favourite float to the top only in the default order. Missing values
  sort last in **both** directions; absent is not a small value
- ✅ `Added` column (`createdAt`) beside the existing `Last run`
- ✅ One-click favourite — the only action performed *while browsing*
- 🔄 Card view parity: it trailed the table by a full restyle twice and had to be
  caught by eye, not by a test

## Phase 6 — Visual language ✅ (settled 2026-08-01)
Authoritative: docs/DESIGN.md. Neutral dark greys (R=G=B), one accent meaning
*live*, one line colour, motion that stops.
- ✅ Surfaces `#121212 / #1a1a1a / #242424 / #2e2e2e`, ink `#d6d6d6` at
  **12.9:1** — past AAA, deliberately short of the ~17:1 that causes strain
- ✅ **One line colour.** `--color-rule` for every separator in the app; the
  near-black seam token was deleted once nothing referenced it
- ✅ Motion via motion.dev: log panel animates **width, not x-position**, so the
  list reflows with it instead of appearing shoved aside. `LazyMotion` +
  `domAnimation` + `strict`
- ✅ Container queries throughout the list, never viewport breakpoints — see
  Phase 10
- ❌ **Per-language identity colours — REJECTED** (2026-07-30). Twelve hues down
  a list is confetti, not a system, and it put colour on the one surface that
  should stay quiet
- ❌ **Blue-violet brand gradient — REJECTED** (2026-07-31). It made the accent a
  decoration you could not point at, and the violet end carried no meaning
- ❌ **Cool-cast greys — REJECTED.** Argued for as "chosen rather than default";
  next to Photoshop and VS Code all day it read as a tint. Now R=G=B
- ❌ **A blanket style blacklist — REJECTED** (2026-07-30, Tanner's correction).
  A generic "no gradients/glass/glow" list had been recorded as his preference
  and was silently narrowing every decision. Every style is available; only
  *SVG-icons-never-emoji* and typography craft are standing rules

## Phase 7 — Monitoring ✅ core · 🔄 depth
- ✅ Whole process **tree** sampled at 1 Hz and summed. Verified on a real
  project: 188 MB across 6 processes, where the spawned pid alone read as a few
  megabytes at 0%
- ✅ CPU is a percentage of one core and is **not clamped** — a four-core bundle
  reads ~400%, which is the truth and the interesting case
- ✅ Listening ports via `GetExtendedTcpTable`, attributed to the tree
- ✅ ONE sampler, ONE coalesced event per tick. Nothing is emitted when nothing
  runs, so an idle Launch Deck is genuinely idle
- ✅ **`System › Machine`** — the host itself: CPU with per-core breakdown,
  memory, GPU, VRAM, disks, and the static machine specs, over two minutes of
  history. The project figure is unreadable without it: a ninety-second build
  means something different when the machine was idle than when another project
  had it pinned.
- ✅ **GPU utilisation via PDH**, the source Task Manager uses — `sysinfo` has
  no GPU support at all. Instances are grouped by engine type (`3D`, `Copy`,
  `VideoDecode`…) and the busiest becomes the headline, because summing every
  instance produces figures over 100% that mean nothing.
- ✅ **Unknown is never rendered as zero.** Every GPU field is nullable end to
  end; absent counters read "unavailable". An idle GPU and an unmeasured GPU
  look nothing alike to someone debugging a slow build.
- ✅ Adapters come from **DXGI, not WMI** — a direct call with no service
  dependency, where `Win32_VideoController` can take seconds on a cold provider
  and would block the calling thread.
- ✅ **Every figure is diffed against Windows' own counters**
  (`tools/verify/stats-accuracy.ps1`, 27 checks): CPU vs
  `\Processor Information(_Total)`, memory vs `Win32_OperatingSystem`, GPU vs
  `\GPU Engine(*)` grouped the same way, VRAM vs `\GPU Process Memory(*)`,
  disks vs `Win32_LogicalDisk`, cores and brand vs `Win32_Processor`. Measured
  agreement: **memory exact to 0 MB**, cores/brand/disks exact, uptime within
  4 s, CPU within 1.4 points. A resource monitor is uniquely easy to get subtly
  wrong and impossible to catch by eye — 38% looks equally convincing whether it
  came from the kernel or from a units mistake.
  - CPU and GPU are sampled **concurrently** with the Windows counters, not
    before or after: a sequential comparison measures two different seconds and
    then blames the difference on the code.
- ✅ **Software adapters are filtered out of the spec list.** The Microsoft
  Basic Render Driver is a Windows fallback renderer DXGI always enumerates;
  listing it under "Graphics" read as a second graphics card the machine does
  not have. It stays in the data, it is just not presented as hardware.
- ✅ **Polls only while the view is open.** No background ticker: a launcher
  that samples the machine every second forever is a launcher that shows up in
  the Task Manager it is imitating. A reading older than 5 s is re-primed
  rather than reported, because a delta covering an hour-long gap is a
  correctly-computed average and a useless "right now" figure.
- ✅ **Health probes: "Running" now means serving, not spawned.** A process
  exists 11 ms after the click, but `npm run dev` needs seconds to bind a port.
  For that window the badge said Running while nothing was listening — right
  about the process, wrong about the service, and people act on it. It reads
  **Starting** until a readiness signal arrives.
- ✅ **Readiness needs no HTTP client and no polling.** The metrics sampler
  already records which ports each project tree is *listening* on, once a
  second, via `GetExtendedTcpTable`. That is a stronger signal than an HTTP
  probe, already collected, and unlike a request it cannot perturb the thing it
  measures. Adding `reqwest` would have meant a TLS stack to learn something
  already on hand.
- ✅ **`Unknown` is not unhealthy.** A script or a build has nothing to bind;
  `healthy` stays `null` and renders exactly like plain Running. Mapping it to
  `true` would claim a health nobody verified; mapping it to `false` would put a
  warning on every batch job behaving perfectly.
- ✅ **Listening on an undeclared port still counts as ready.** `default_ports`
  are guesses — `node.toml` lists 3000/5173/8080 because Node projects commonly
  use *one* — so insisting on the guessed port would strand a project on
  "Starting" forever.
- ✅ Verified end to end against a fixture that binds only after 4 s: badge read
  Starting for four ticks, then Running the second the port appeared.
- ⏳ [S] Disk and network throughput are sampled but not surfaced

## Phase 8 — Diagnostics ✅
`System › Diagnostics`. **It judges rather than dumps** — a screen that prints
forty facts makes the reader do the diagnosis, so its usefulness is capped by
their memory of what each value should be.
- ✅ 16 checks across 13 sections: database integrity, crash-safe write settings,
  WAL-vs-database size, foreign keys, recovery snapshot, runner manifests, log
  pumps vs live processes, log disk, data-directory writability (probed by
  *actually writing a file*), toolchain on PATH, boot time, launch time, memory
- ✅ The database probe reads from the **live connection**, never repeating the
  constants in `store.rs`. A report that echoed its own configuration could not
  have caught the setting that lost the registry
- ✅ **It found a real bug on its first run**: corrupt indexes on `runs`.
  Confirmed from an independent sqlite3 connection, repaired with `REINDEX`
  after a backup. The corrupt index made `COUNT(*)` return 8 where a forced
  table scan returned 3 — **no rows were lost; the 8 was the lie**
- ✅ Catches `NoDefaultCurrentDirectoryInExePath`, the Git Bash export that
  breaks `.bat` launches for inherited children
- ✅ Nothing here mutates state, kills a process, or repairs anything. A
  diagnostic that also fixes things cannot be run safely when you do not yet
  know what is wrong
- ⏳ [S] A guarded "Repair indexes" action, so a finding is actionable in-app

## Phase 3e — Are the commands actually right? ✅ (audited by launching everything)

*"Some programs are broken and don't launch with flawless logs."* Reading thirty
runner manifests to guess which is the slow way to be wrong, so
`tools/verify/launch-all.mjs` presses Run on every project, waits, and records
the state plus the tail of that project's own log. First run: **17 of 32 did
not launch.** Sorted into three piles, which is the useful part:

**Ours, and fixed:**
- ✅ **Stale runners are re-detected at startup.** Detection freezes at
  registration, and runners are DATA — dropping in a better manifest is the
  supported way to teach Launch Deck something. Pulse was registered before the
  Tauri manifest could claim it, so it kept running `cargo run` in a workspace
  with two binaries and failed with *"available binaries: pulse, pulse-app"*
  every single time. `redetect_project` already fixed that, from a menu nobody
  had a reason to open. Now it runs for every project on startup, in the
  background, and logs each correction.
- ✅ **A packaged Python project no longer offers a Run that cannot work.**
  Video-Forge has `pyproject.toml`, a `videoforge/` package, no `.py` in the
  root and no `__main__.py` — so `entry` fell through to its default and the
  Run button executed `python main.py` on a file that does not exist.
  `can't open file 'main.py'` is a worse answer than *"this needs installing"*,
  which is the true one. A `[[setup]]` rule now says so.
- ✅ **Setup markers may be globs.** The rule above needs `*.egg-info` to know
  a package is installed, and `exists()` was a literal path check — so the hint
  would have persisted after a successful install, which is its own kind of lie.

- ✅ **Python failures are diagnosed.** Every rule in `detect_exit_reason`
  matched Node's phrasing — "cannot find module", "module not found" — and
  Python says *"No module named 'mutagen'"*, which matches none of them. Both
  Python failures in this library therefore landed as a bare "Exited with code
  1". The rule now names the module, because `pip install mutagen` is a command
  and "a dependency is missing" is a shrug. A dotted path reports its first
  segment: `pip install google.protobuf` is not a thing.
- ✅ **Run is disabled when the project's folder is gone.** `rootExists` already
  suppressed the Install button and did not touch Run, so a deleted folder
  offered a launch whose only outcome was `The directory name is invalid.
  (os error 267)` — an error about a syscall, for a situation the app already
  knew about. The tooltip now says "Folder no longer exists".

**Not ours, and correctly reported:** `cmake` missing for Strem and Vesper,
`go` missing for Youtube-Comment-Parser (a Go project that lives in `Dev/Python/`
— detection is right, the toolchain is absent), and `mutagen`/`whisper` missing
for two Python projects that declare no `requirements.txt` at all. Every one of
these already produced a log line naming exactly what was missing.

**Neither, and my own mess:** `ZZ-Close-Fixture` pointed at a deleted temp
directory, because `survives-close.ps1` registered a fixture project and never
removed it. Every later audit reported it as a broken project — noise the
harness created and then blamed on the app. It cleans up after itself now.

- ❌ REJECTED — guarding `tauri-built`'s workspace glob on `crates/` not
  existing. It fixed Pulse and **regressed Launch Deck itself**, which is a
  workspace whose app genuinely builds to `target/release/launch-deck.exe`;
  the guard sent it back to `npm run tauri dev`, the slow path that manifest
  exists to avoid. Reverted the same hour it was written.
- ✅ **Glob patterns in `[[vars]]` can reference variables**, which is what
  finally fixed binary selection properly. A manifest can now say *the binary
  named after this package* instead of *any binary in this directory*:

  ```toml
  [[vars]]
  name = "appname"
  from_value = { toml = { file = "src-tauri/Cargo.toml", path = "package.name" } }
  default = "*"

  [[vars]]
  name = "app"
  first_match_any = [
    "src-tauri/target/release/{appname}.exe",
    "target/release/{appname}.exe",
    "src-tauri/target/release/*.exe",
  ]
  ```

  Vars resolve top to bottom, so a pattern may only use one declared above it —
  the same rule command templates already follow — and an unknown name leaves
  the pattern unexpanded so it matches nothing rather than everything. That
  second property has its own test, because a pattern that quietly degraded to
  `*` would reintroduce the exact bug.

  **There is deliberately no `target/release/*.exe` last resort.** That
  directory holds every workspace member's binary, and guessing is how Pulse
  came to launch its `pulse` CLI, watch it print usage, and report success.
  With no guess available the launch now fails loudly instead, which is the
  honest answer when the app has not been built.
- 🟡 **Residual: an unbuilt Tauri app inside a workspace reports a confusing
  error.** `tauri-built` still claims it — detection sees *some* executable —
  and then `app` resolves to nothing, so the message names the project root:
  ``` `D:\Workspace\Dev\Rust\Pulse/` was not found on PATH ```. Correct in
  substance, poor as a sentence.

  Fixing it properly means letting **detection** rules interpolate variables
  too, and `rule::evaluate` is shared by detection, variable cases and setup
  rules — so that is a wider change than the problem justifies for one project
  in a state that resolves itself the first time it is built. Written down
  rather than done.

## Phase 9 — Performance ✅ measured · recurring
Instrumentation is permanent and visible in Diagnostics, because "it feels
slow" is not actionable.
- ✅ **Launch, perceived: 41 ms → 17 ms.** Both halves measured on the *same
  click* — the optimistic transition, and the moment the badge reads "Running",
  which only the backend can produce. One measurement, so there is no comparing
  across machine states.

  The row flips to `starting` on the click instead of after the round trip. The
  backend's own share of a warm launch is **6–16 ms**; the rest was IPC and
  re-render, which is real work but not work the user has any reason to wait
  through — the click already committed the decision. `starting` is a state the
  run-state machine already has and exactly what the backend is about to
  report, so this shows the truth early rather than a guess. It must never
  claim `running`, which would assert a process exists when none does.

  The rollback is the load-bearing part: a spawn failure emits `Crashed` and
  corrects itself, but `AlreadyRunning` and a required-port conflict return
  *before* anything spawns and emit nothing at all. Without restoring the
  previous state those rows would sit at `starting` forever — a stuck spinner
  claims something is happening, which is a worse lie than the wait it replaced.
- ✅ **Cold launches are the OS, not us — and mostly not recoverable.** A binary
  the OS had not seen recently costs **~800 ms** against 6–11 ms warm. Raw
  `Process.Start` on the same warm exe is 4–5 ms and a freshly started Launch
  Deck launches it fast, so none of the cold cost is ours.
- ❌ REJECTED, and this one was **claimed before it was properly measured**:
  "reading the binary first recovers most of the cold cost — 630 ms → 149 ms".
  It does not. That experiment used *different binaries in each arm* — odd
  index plain, even index primed — and the primed arm happened to contain two
  that were already warm. A selection artifact reported as an effect.

  Done properly, with **fresh copies of one binary so both arms hold identical
  bytes**, each copy executed exactly once, arms alternating:

  | | median | max |
  |---|---|---|
  | plain | 868 ms | 900 ms |
  | primed | 815 ms | 869 ms |

  **6%.** Five of six pairs favour priming, so the effect is real — it is just
  small, because the cold cost is dominated by first-execution scanning, which
  happens at process creation and is not satisfied by having read the file.
  The in-app A/B agreed all along and was disbelieved: no difference across
  sixteen projects.
- ✅ **`Prewarmer` kept, but cut back to what it earns.** Hover-driven priming
  is **removed**: hover is continuous — crossing a list of thirty rows touches
  every one — so its cost scaled with mouse movement while the benefit stayed
  at 6%. Reading hundreds of megabytes to maybe save fifty milliseconds is the
  wrong trade, and debouncing changes the constant, not the ratio.

  What remains is selection, plus the five most recently launched projects at
  startup. That is bounded (**7.6 MB**, deduplicating five projects to three
  distinct programs, inside the first 1.2 s, boot unchanged at 734 ms against
  741 ms) — and it does something the 6% story missed entirely: priming
  *resolves* each program, which populates the PATH-resolution cache below.
  **That** is worth 2.2–4.2 ms on every launch, and it is what justifies the
  call. The 6% rides along.
- ✅ **The spawn phase is attributed, and one line of it was 50× too slow.**
  `spawn` was a single opaque number — the same shape of thing that hid the
  1120 ms setup regression. Split into log-sink / job-object / resolve /
  `CreateProcess`, and the answer was immediate: **resolving a bare program
  name cost 2.2–4.2 ms of an ~11 ms backend launch**, because `npm` walks all
  43 PATH entries against every `PATHEXT` suffix, every single time, for an
  answer that cannot change inside a process.

  Cached per process, revalidated on each hit with one `is_file` so a tool
  uninstalled while the app is open falls back to a fresh search. Misses are
  never cached — install Node, press Run again, and it must work without a
  restart. Measured in a real launch: **npm 4199 µs → 78 µs, powershell
  2229 µs → 74 µs**. Absolute paths were already 45 µs and are untouched.

  A pleasant accident: startup prewarming calls `resolve` for the five most
  recent projects, so their first launch of the session already finds a warm
  resolution cache as well as a warm file cache.
- ❌ REJECTED — "our spawn flags are what makes big binaries slow". They are
  not. A dedicated harness (`cargo run -p deck-runtime --bin spawnflags`) times
  the exact combination we ship against a plain spawn: **every combination is
  3.5–4.6 ms**, including `CREATE_SUSPENDED` on a 13.5 MB binary. The
  suspension that closes the job-object race is free, which also settles the
  standing question of whether `PROC_THREAD_ATTRIBUTE_JOB_LIST` would buy
  anything — it would not.

  The 306–474 ms readings that prompted this were **my own instrumentation**:
  `t_create.elapsed()` was read at the log line, which is after the job
  assignment, the resume and two reader tasks. Correctly placed, `CreateProcess`
  for the same binaries is 6.1–6.2 ms. Fifth measurement error of the session,
  and the first one caught before it reached a claim.
- ✅ **The `CreateProcess` outlier is just the cold-binary cost, and my first
  explanation of it was wrong.** It was recorded here as "specific to Launch
  Deck launching its own image" on the strength of one run where only
  `launch-deck.exe` was slow. The next run had Chess-Scout at 455 ms and
  Claude-Advisor at 320 ms — the same binaries that had measured 6 ms an hour
  earlier, because they had been launched minutes before. There is no
  self-image effect: `spawnflags --self` measures 3.4 ms against 3.2 ms for a
  different image.

  It is one phenomenon, not two: **the first launch of any heavy binary once
  the OS has gone cold on it costs 300–800 ms; every launch after costs
  6–36 ms.** Not the binary (4.6 ms from a standalone harness), not our flags
  (3.5–4.6 ms for every combination), not sequence position. It is the OS, it
  is mostly first-execution scanning, and 6% of it is recoverable — see above.
- ❌ REJECTED — "prewarming makes launches faster *in this library*". An in-app
  A/B across sixteen untouched projects found **no difference** (plain 78 ms
  median, primed 73 ms). Most projects here launch `node`, `python` or `cmd`,
  which are permanently resident on any machine that uses them, so there is
  nothing to warm. This result was correct and was disbelieved for several
  hours because a confounded experiment disagreed with it.
- ✅ **Boot: 484 ms** process start → project rows painted, measured **without a
  debugging port attached**. The long-standing "~930 ms" figure was measurement
  distortion: enabling CDP roughly doubles WebView2 startup, so every
  CDP-instrumented boot number this project ever recorded described a boot no
  user experiences. It is now reported by the frontend calling `ui_ready` after
  the first paint, and logged so it can be read from an uninstrumented run.
- ✅ **Setup: 1120 ms → 18 ms**, of which **0.1 ms is unattributed.** The
  regression was `HostMonitor::new()` running inside `setup()`: the first
  `PdhOpenQuery` loads the entire performance-counter registry (**254 ms
  standalone**, far worse under contention with WebView2 starting). It is now
  built lazily and warmed on a background thread, so the Machine view is still
  instant and nobody else pays for it.
- ✅ **Diagnostics now names which phase is slow.** The report said
  "Setup total: 1120 ms" and nothing else — a total that cannot be attributed is
  a log, not a diagnostic. There is now a per-phase breakdown plus an
  `unattributed` line and a check that fails past 120 ms, so the next thing
  added to setup without instrumentation announces itself.
- ✅ **The boot budget is now split end to end**, from process start through
  `Builder::build()`, setup, and four webview marks (HTML parsed → our JS runs →
  React mounts → rows painted), plus the project-list IPC. Where the time goes,
  measured on an idle machine:

  | Phase | ms | Ours? |
  |---|---|---|
  | process start → the page's clock starts | ~372 | **no** — WebView2 |
  | page clock → **first byte of `index.html`** | **~304** | **no** — see below |
  | first byte → DOM interactive | ~12 | yes |
  | serving the JS bundle (422 KB) + CSS | **19** | yes |
  | project-list IPC round-trip | ~32 | mostly transport |
  | render → rows painted | ~55 | yes |
  | `Builder::build()` / our `setup()` | 6 / 18 | yes |

  **~88% of boot is WebView2 starting a browser engine.** The finer split above
  replaced an earlier one that had "bundle fetch + parse ~30 ms" doing work the
  ~304 ms row was actually doing.

  That row is the whole story and it took a specific measurement to find:
  `Resource Timing` on the **original navigation** — not a reload — shows the
  document's own first byte arriving **303.9 ms** after the page's clock starts.
  The same request on reload takes **2.0 ms**. Nothing about the document
  changed; WebView2 simply is not yet able to serve a request for a file
  embedded in our own binary. Our protocol handler, once the engine is up,
  serves 422 KB of JavaScript in 12 ms.

  Measuring the reload alone made the handler look like the whole story, and it
  is not the story at all. `tools/verify/asset-timing.mjs` runs both, because
  they answer different questions.
- ❌ REJECTED, measured — **WebView2 startup flags.** WebView2 is Chromium and
  takes Chromium's command line, so disabling startup subsystems a local-only
  window has no use for looked like free milliseconds. Four configurations,
  five interleaved rounds each, the app's own `ui_ready` figure:

  | flags | median | min |
  |---|---|---|
  | none | 771 ms | 744 ms |
  | no background networking / sync / component update | 752 ms | 742 ms |
  | + no OOUI, no SmartScreen | 740 ms | 719 ms |
  | + no renderer backgrounding | 754 ms | 706 ms |

  A 31 ms spread across every configuration, against a run-to-run range of
  656-906 ms already established for boot. That is inside the noise, and buying
  nothing is not worth turning off a security feature for. Recorded so the next
  person to have this idea can skip it.
- ✅ **`list_projects` is 1 ms server-side** (`db_ms=0, map_ms=1` for 32
  projects, including resolving every runner's command template). The ~45 ms
  round-trip is Tauri IPC transport, not query cost — which was worth measuring,
  because the obvious suspect was the per-project `plan()` resolution and it was
  innocent.
- ✅ **Add/scan dialogs are code-split** — they were imported eagerly and
  rendered unconditionally, parsing their form and scan code on every boot to
  display nothing until clicked. **6.4 KB off the main chunk** (517.9 → 511.5),
  verified end to end: no chunk fetched at boot, fetched on open, dialog renders.
- ✅ **motion/react removed: 79.8 KB off the main chunk** (511.5 → 431.7), the
  single largest cut so far. It was **15.5% of the bundle**, attributed by
  walking the sourcemap and charging generated bytes to their source file
  rather than guessing from package sizes. What it bought: three transitions —
  the log panel, the detail bar and toasts — all animating opacity, size and
  transform, which the browser does natively and off the main thread. The
  runtime was parsed on every boot including the majority where nothing ever
  animates.

  Only one part was genuinely hard, and it is 130 lines in `lib/presence.ts`:
  React removes a node the instant it stops rendering it, so there is nothing
  left to animate on the way out, and the *data* disappears too — `logProject`
  is already null when the panel starts leaving. The hook holds both. The
  animation itself is `@starting-style` plus `interpolate-size` in `tokens.css`,
  on the same duration and easing tokens, so timing is unchanged rather than
  merely similar.

  Guarded by `tools/verify/motion-verify.mjs`, which samples the animated
  property every frame and asserts it passed through intermediate values. A
  jump cut ends in the right place and would pass any static check — sampling
  is the only way to tell a transition from a teleport.

  **CORRECTION — the boot win was overstated.** The commit that landed this
  claimed "first paint 151 ms sooner". It is not supportable. That number came
  from comparing an after-run taken while a game held ~50% of the CPU against a
  baseline taken on a quieter machine, and load does not simply scale the total
  — it moves work *between* phases. A contended WebView2 takes longer to start,
  the asset fetch overlaps that longer window, and so more of the frontend is
  already done by the time the page's clock starts. That run showed
  `dom_interactive` at 62 ms where both the baseline and every later idle run
  show ~280 ms.

  Measured properly, at 2% CPU with a pre-page phase of 372 ms against the
  baseline's 373 ms — the same machine state, which is the entire point —
  **741 ms against 778 ms**. About 37 ms, with per-run ranges that overlap
  heavily. So the truthful claim is: the bundle is **79.8 KB smaller, verified
  from the build output**, and the boot effect is at most a few tens of
  milliseconds and is not separable from run-to-run variance on this rig.

  Not a reason to put the library back — 80 KB less to ship, parse and keep
  working is worth having on its own. It is a reason to distrust any boot
  comparison whose two halves were not measured under the same load, which is
  now enforced: `boot-real.ps1` prints the CPU load beside every result and
  warns past 25%.
- ❌ **Prefetching the project list at module scope — REJECTED, measured.** It
  did move the request 21 ms earlier (before React mounts), and the data still
  arrived at the same ~120 ms: the round-trip simply grew by what the head start
  saved. The constraint is a fixed readiness point in the webview, not when we
  ask. Reverted rather than kept, because it cost a duplicated `queryFn` that had
  to stay in sync with the hook — a real footgun bought for nothing.
- ✅ **Icons: 29 IPC round-trips → 1.** `project_icon` was keyed per row, and
  each call did its own database read plus a filesystem probe. Batched into one
  pass on the blocking pool. Worth doing, but honestly measured: it moved first
  paint by ~9 ms (inside noise) because `ui_ready` fires when the *project list*
  arrives, before icons load. It improves the phase after the one benchmarked.
- ✅ **Launch: 11 ms mean** click → child spawned — 63 → 43 → **11 ms**. Lookup,
  planning and the port check total **under 0.5 ms**; the spawn itself is now
  the whole figure. Measured end to end through the release build's own
  instrumentation, not in a micro-benchmark.
- ✅ The launch path read the whole TCP table **twice** per launch; one snapshot
  now serves both checks, and projects declaring no ports skip the syscall
- ✅ Diagnostics is code-split so its chunk is not parsed on every boot
- ✅ **The 32 ms that was one call is gone.** `resume_process` located the
  child's main thread with `CreateToolhelp32Snapshot`, enumerating every thread
  on the machine — a standing test clocked it at **33.3 ms over 5,923 threads**,
  three quarters of the entire launch. It now resumes through `NtResumeProcess`
  on the handle already held: **0.01 ms against 73.8 ms**, measured side by side
  in `handle_resume_beats_the_thread_walk`.
- ✅ **The undocumented call is a bounded risk, not a taken one.**
  `NtResumeProcess` is absent from the Win32 headers but is what `kernel32` is
  built on, unchanged since NT 3.1. The symbol is resolved at runtime and the
  ToolHelp walk remains as a fallback, so a missing symbol would cost speed,
  never a launch. A test asserts the fast path is the one actually running —
  without it, a silent lookup failure would leave the optimisation as dead code
  that every other test still passes around.
- ✅ **Two tests guard the behaviour, not just the speed**: one proves a resumed
  child actually executes its command (a resume that returns `Ok` while leaving
  the process suspended is indistinguishable from a working one until a user
  watches a project hang forever), and the existing job-object suite still
  proves a grandchild dies with its parent.
- 🔄 Rows use `content-visibility: auto`, not a windowing library. Enough for
  fixed-height rows at this scale, but it does **not** virtualize the React
  tree — revisit with `virtua` past a few thousand projects

## Phase 10 — Layout robustness ✅ (a whole class of bug closed)
- ✅ **Container queries, never viewport breakpoints.** With the log panel open
  on a 1240 px window the list had ~470 px, so `lg:` variants fired for a
  container that could not fit half its columns: headers stacked and the action
  buttons left the visible area
- ✅ Responsive is a **declared priority order**, not shrinking. Never hidden:
  status, name, primary action, overflow menu. Everything else sheds at a stated
  container width, and **anything that sheds gains a menu entry** — a control
  hidden at some sizes and absent from the menu is a feature that silently does
  not exist
- ✅ The name column has an **80 px floor**, asserted at four widths with the
  panel open and closed (34/34 geometry checks)
- ✅ **The gitignore trap**: `logs/` in the workspace `.gitignore` matched
  `src/features/logs/`. Tailwind v4 skips gitignored paths, so every class used
  only by the log panel generated **no CSS at all** — which is how the panel lost
  its `max-width` and grew unbounded. Fixed by re-including the directory *and*
  declaring `@source` explicitly

## Phase 11 — Accessibility ✅ (audited by driving the keyboard)
- ✅ `color-scheme` tracks `data-theme`, so native scrollbars and carets follow
  the app rather than the OS
- ✅ Focus is an **inset ring** on rows — a background change cannot indicate
  focus on a row that is already selected, and keyboard users routinely have
  both at once on different rows
- ✅ `aria-sort` on sortable headers · icon buttons named · toasts announced ·
  dialogs contain their own scrolling · `prefers-reduced-motion` honoured
- ✅ **Keyboard traversal of the dialogs, audited by driving real keys** —
  `tools/verify/a11y-verify.mjs`, 21 checks. Focus enters on open, Tab is
  trapped (walked past the control count to prove it wraps rather than leaks),
  every control is reachable, the focus ring is measured from computed styles,
  Escape closes.
- ✅ **Escape used to drop focus on `<body>`.** Radix restores focus to whatever
  was focused when a dialog opened — and both of these open from an item inside
  a dropdown, which is unmounted by the time the dialog closes. So there was
  nothing to restore to: measured, `document.activeElement` was BODY, meaning
  the next Tab restarted from the top of the document and a keyboard user lost
  their place completely. Fixed once in the shared `Dialog`, which both use.
- ✅ **Diagnostics is fourteen navigable regions instead of one wall.** Every
  `<section>` had a heading and no accessible name, and an unnamed `<section>`
  is not exposed as a landmark at all — so the structure existed visually and
  not for a screen reader. `aria-labelledby` points each region at the heading
  it already had, rather than duplicating the title as a string that can drift.
- ❌ REJECTED for this harness — contrast checks. `guidelines-verify` already
  measures contrast from computed styles, and two suites asserting the same
  thing drift apart. It also does not judge whether a label *reads* well: it
  can see that a control has an accessible name, not that the name is any good,
  and pretending otherwise is how "has a label" gets called accessible.

## Phase 17 - One app for all my apps (2026-09-07)

Tanner's brief, in his words: *"continue to refine this and make it the all in
app for all my apps i mqde and integrate other apps into this app"*, then
*"improve the launch commands also allow me to launch web pages through there i
want instant access to my apps also allow for different views aswell ... add a
function to see what projects take up the most space ... and have the bars be
different colors depending on how much space they take up also let me click on
the folder and expand to see what subfolders take up the most aswell"*, plus
*"a little like equivlant to a man command but more like explain how to use it,
how to launch it, what it can do, whats its purpose ... all organize in a right
side pannel like spotify"*.

### 17a - Facts, not just reachability (done)
The status chip could say a source *answered*. It could not say what it said.
`deck-runtime::facts` adds per-app readers that turn an answer into glanceable
values: Fleet's active runs / armed teams / five-hour plan window, Ollama's
installed and loaded model counts, Lathe's Ollama health + model count +
request total, FocusForge's play credit / flow / priority gate, and the Pavlok
status file's last alert. Chosen per project in the Status source dialog.

- **The first version of every reader was written against a guessed payload,
  and every guess was wrong.** It looked for `status` on Fleet's health
  endpoint (which reports `ok`, and nests `status` under `limits` meaning
  something else entirely), for `todayFocusMinutes` on FocusForge (which
  reports `balance_min`, `flow` and `gate`), and for scalar counters on Lathe's
  `/stats` (which is a map of nested per-route objects). All three returned an
  empty vec against the live services - and because an empty vec is *also* the
  honest answer for "not running", nothing looked broken. Rewritten from real
  curl output, with the captured payload pasted above each reader and reused as
  the test fixture. **Rule: a reader that has never seen its own service's
  output is decoration.**
- A non-2xx body is still parsed: Fleet answers 503 with the *same* JSON when
  it has problems, which is exactly the case worth reporting.
- `PingOutcome::Responded` now carries `content_type`, so an API endpoint is
  never offered as an embeddable page.
- Verified against the live services, not only fixtures: Fleet, Ollama, Lathe
  and the Pavlok file all returned real values; FocusForge correctly returned
  nothing, because it has been down since 2026-07-30.

### 17b - Launch web pages (done)
A 32nd runner, `web`. "Add a web app" writes a real Windows `.url` shortcut
into a folder under the app data directory and registers it, so a saved web app
is an artefact on disk - it survives a lost database and opens from Explorer -
rather than a row that exists only in SQLite.

- Run does **not** spawn. `useLaunch` is the single place that decides what Run
  means, because there are now four controls that launch a project, and routing
  a web app through the shell would record a run that died a fraction of a
  second after it began - filling the activity list with deaths that never
  happened and the overnight report with failures that never were.
- The row menu now also offers the runner's *other* lifecycles (dev, build,
  test) where the runner really supports them, read from `supported` rather
  than assumed.

### 17c - Two views (done)
The grouped list is the working view. The grid is a dense wall of marks for
"open the one I am picturing" - it drops the supporting line, keeps one status
signal, and makes the whole tile the control.
- First version used a container-query breakpoint ladder and rendered three
  near-empty 210px tiles on a 1240px window. Replaced with
  `repeat(auto-fill, minmax(108px, 1fr))`: the browser fits as many columns as
  genuinely fit, which is the whole job. 12 tiles visible became 30.

### 17d - Storage (done)
Every project measured, sorted either way, a coloured bar per row, and
recursive drill-down into subfolders.
- **Measured one project at a time on purpose.** A single sweep of every root
  ran past five minutes on a cold cache with no output, and there is no honest
  progress bar for a walk whose size is unknown until it finishes. Per-project
  queries fill the list as answers land. Warm, a project root measures in
  11-149 ms.
- **Junctions are skipped, not followed.** There are 28 directory junctions
  under `Dev/Python/Soundcloud-Downloader/Radio/` pointing back into their own
  project; following them counts that tree once per junction, and a junction
  aimed at an ancestor would recurse until the stack gave out. Entries are
  stat-ed without traversing and skipped on `FILE_ATTRIBUTE_REPARSE_POINT`.
  They are still listed, reporting zero rather than their target's size.
  *(This originally cited `Dev/JS/CouponHunter` as the example. That path does
  not exist -- written from memory, not from `dir /AL`. Same failure the facts
  readers had, in the commit that documented it.)*
- Bar **length** is relative to the largest project; bar **colour** is
  absolute, so the same 30 GB is the same red whether or not something bigger
  sits above it. Thresholds use the same 1024 base `formatBytes` renders, so a
  band boundary matches the number printed beside it.
- Unmeasured rows sink to the bottom in *both* sort directions: they are not
  "zero bytes", they are "not known yet".
- First real answer: the library is **149.9 GB**, of which Launch Deck itself
  is 33.1 GB and its `target/` alone is 32.9 GB.

### 17e - The manual panel (done)
A persistent 340px right pane that follows the selection: what the project is,
how to launch it, what commands it declares, where its docs are.
- **Everything is read, never generated.** The summary is the project's own
  README first paragraph with title and badge furniture skipped; the launch
  line is the command the runner resolved; the extra commands are the scripts
  really declared in `package.json` or `Cargo.toml`. A generated description
  would have looked better on every project and would have been a plausible
  guess that ages badly and cannot be fixed by editing the repo. A project with
  no README says so.
- Hidden below a 1100px viewport, where it would leave the library too narrow
  to read. That gate is a **viewport** query: the split declares no
  `@container`, so the `@min-[1100px]` variant the first version used matched
  nothing, and the panel would never have appeared at all.

### 17f - Both reviews said HOLD, and the harness could not tell

A UI finish-gate review and an accessibility audit ran against the shipped
binary and returned HOLD / DOES-NOT-CONFORM, between them naming eighteen
measured defects. Fixing them exposed a nineteenth, in the thing meant to catch
them.

**`reporter().finish()` never set an exit code.** It RETURNED 0 or 1, and the
two suites added this September (`shell-verify`, `regress-verify`) call it as a
bare statement. Both therefore exited 0 whatever they found, and
`run-all.ps1` -- which decides PASS/FAIL from `$LASTEXITCODE` -- printed
**PASS** beside a suite whose own last line read `4 of 27 FAILED`. Fixing it is
how the four real `shell-verify` failures in this work were found.

**Correction, after a third review:** the first version of this entry claimed
every suite had always exited 0 and that no "all suites passed" line in the
project's history could be trusted. That was wrong. The eleven older suites
each end with their own `process.exit(...)` -- checked one by one at `958bcaf`
-- so the blind spot was two suites wide and one commit long, not the project's
lifetime. Setting the code inside `finish()` is still the right fix, because it
makes the reporter self-sufficient and stops the next suite inheriting the bug.

**The grid tile was one shape causing three defects.** A `role="button"` div
wrapping a full-bleed `bg-panel/85` overlay that held the Run button, because
HTML forbids a button inside a button:

- The overlay was `opacity: 0` but **not** `pointer-events: none`, so an
  invisible Run button owned the centre 24.6% of every tile.
  `elementFromPoint` at a hovered tile's centre returned it, and the reviewer
  launched two projects by accident while reviewing. The file's own comment
  claimed "single click selects... the same contract the list row uses".
- The overlay became opaque exactly on hover and focus, **erasing the mark on
  the one tile you were pointing at** -- on a screen whose stated premise is
  that recognising a mark beats reading a name.
- The focus ring was an **inset** box-shadow, which paints on the padding box
  *below* descendants, so the overlay covered it: **1.22:1** measured from
  screenshot pixels, while `getComputedStyle` truthfully reported
  `inset 0 0 0 2px accent` the whole time. **That is why one review passed it
  and the other did not** -- one read the style, one read the pixels.

Rebuilt as a real `<button>` with the action as a corner sibling.

**The check written to guard it was itself vacuous**, and a third review caught
that too. "Nothing is stacked over the focused tile" read
`stack[0] === tile || tile.contains(stack[0])` -- and the overlay that caused
the defect was a *descendant* of the tile, so `contains(...)` was true by
definition and the check passed against the exact bug. It is now identity only,
joined by a real contrast measurement, because "the ring is declared" and "the
ring can be seen" are the two things this whole episode is about. Measuring
that took two more instrument bugs: Chrome's canvas `fillStyle` silently drops
the alpha from `oklab(... / 0.15)`, so a 15% tint resolved as solid accent and
the ring read 1.02:1 against a background supposedly its own colour; and the
resolver, being a JS template literal, had its regex backslashes eaten before
the page saw them. Corrected, the ring measures **4.57:1** on a selected tile,
against a 3:1 requirement.

**Everything else fixed and now guarded by `regress-verify` (20 checks):**

| Was | Now |
| --- | --- |
| 50 tiles = 100 tab stops, 134 tabbables in the document | roving tabindex: 1 stop, 35 tabbables |
| ArrowDown scrolled the pane 36px, focus unmoved | arrows move in two dimensions off the live column count |
| Enter launched, Space selected, nothing announced the difference | both select, as on a list row |
| 32px mark = 8.3% of the tile | 44px |
| search-then-Enter dead in grid view (looked only for `[data-project-row]`) | matches both view shapes |
| parent bar `w-full` vs child bar `w-24`: `target` at 99% of its parent drew 1/9 the length | parent bar withdrawn while expanded; children full width |
| a 2px floor made 18 of 20 children identical | true proportion, no floor -- and no suppression either, which blanked 31 of 50 top-level rows when tried |
| manual toggle `aria-pressed="true"` while the pane was `display: none` | absent below 1100px and where no project can be selected |
| both dialog fields pointed at one error node, no `aria-invalid` | per-field, and focus moves to the offending input |
| `docs.slice(0, 8)` dropped the rest silently on 8 of 51 projects | states how many more |
| "Also supports: run only" | "Run only - this project declares no dev, build or test step" |
| manual actions 574px below the fold | directly under the identity |
| `formatBytes` printed "0 KB" for a 400-byte file | "<1 KB" |
| four unnamed `<section>`s in the manual | `aria-labelledby`, which this project's own a11y suite already required |
| `open_path` / `folder_children` gated with a component-wise `starts_with` | canonicalised -- `..` is a component, so `<root>\..\..\CLAUDE.md` passed the old check and reached `D:\Workspace\Career\` |
| `register_web_app` accepted control characters into a shell-parsed `.url` | CR/LF rejected before the scheme check |

**Two findings deliberately NOT fixed, because they are Tanner's call.** Both
are app-wide, both predate this work, and both follow values he set on purpose:

- **White on the accent measures 3.39:1** in dark mode (`--color-on-accent`
  `#ffffff` on `--color-accent` `#4b8fd6`), at 13px/600 and 15px/600. AA wants
  4.5:1. Fixing it means darkening the accent to about `#3a74b0` -- and he
  tuned that accent himself after calling the previous one oversaturated.
- **Light-mode `ink/60` measures 3.30-3.44:1**, which hits every secondary
  label in the app. It is faithful: `--color-ink: #3c3c43` is Apple's published
  `secondaryLabel`, and Apple's own value fails WCAG. Fixing it properly means
  making light-mode `--color-ink` a primary near-black so its opacity steps
  derive correctly, the way dark mode already does (dark `ink/60` measures
  5.95:1). New code uses `/70` and `/75` to clear the bar within the existing
  palette; the systemic fix is a palette decision.

### What the new suite caught that a screenshot could not
`shell-verify` (27 checks) drives all four surfaces. Two of its own bugs are
worth recording, because both produced green:
- A poll loop waiting the full 10s made the CDP *call* time out rather than the
  check fail, so the harness reported a timeout instead of the assertion.
- The sidebar's "All projects" button renders its label **and** its count, so
  `textContent.trim() === "All projects"` never matched. The suite silently
  stayed on Storage, and three manual-panel checks then passed against a
  project selected earlier - green, and proving nothing. It now navigates
  through `showProjects` and asserts the panel names *the row that was clicked*.

## Phase 18 - The launcher audit (2026-09-14)

Tanner's brief opened in capitals: *"ENSURE EVERYTHING LAUNCHES JUST FINE
REGARDLESS OF LANGUAGE"*, then asked for a full UI/UX audit, reliable
launching, GUI flag management, junk filtering, non-project launch items, hub
features, and -- explicitly -- **no bloat**: *"Do not add features simply
because they are possible."*

### What the audit found, from the data rather than by eye

The registry and 475 real run records were read before anything was changed:

| Finding | Number |
| --- | --- |
| Registered entries | 51, growing to 57 during the session |
| Favourites / pins / categories / tags | **0 / 0 / 0 / 0** |
| Never launched from here | 24 of 51 |
| Currently failing | 4 |
| Folder gone entirely | 1 (Switchboard) |

**Nothing had ever been organised**, and the resting sort was pinned ->
favourites -> name. Both tiers were empty, so a 56-project launcher opened
alphabetically while 475 run records described exactly which ones mattered.

The four failures sorted into five causes, and only three were Launch Deck's:
a wrong entry point (Video-Forge has no `main.py`), an ambiguous binary
(Pulse), missing dependencies (Lyzee had no `node_modules`, so `vite` was not
on PATH) -- plus broken project code and a project needing browser cookies,
neither of which a launcher can fix.

### 18a - Readiness before the click, not a log after it

`deck-runtime::preflight` plus an extended `setup_reports`. Filesystem reads
only -- never an execution, the same discipline detection follows, because
this runs over the whole library automatically. On the real registry, **16 of
56** projects report a specific cause:

| blocker | meaning | repair offered |
| --- | --- | --- |
| `root` | the folder is gone | point it at the new folder, keeping history |
| `setup` | dependencies missing | the install command, one press |
| `program` | not installed / not on PATH | edit launch options |
| `entry` | the command names a file that is not there | edit launch options |
| `ambiguous` | cargo, several binaries, no default | pick one, saved as `--bin x` |
| `unbuilt` | a built artefact the runner launches is absent | run the build step |

Plus `lastFailedExit` from run history, for what no static check can see:
Transcriber fails on `import whisper` and failed **fourteen times in a row**
with nothing on the row to say so, because a run's state resets on restart.
That is labelled "last run failed", never "this will fail" -- it is evidence,
not prediction.

**Two of these needed real shapes, not guessed ones.** Pulse is a *virtual*
workspace (`[workspace] members`, no `[package]`) and the first ambiguity
check bailed on the missing package -- so the one project that demonstrated
the bug was the one it could not see. And a program resolving to
`D:\Workspace\Dev\Rust\Pulse/` is not a PATH lookup failing: that trailing
slash is a runner variable that resolved to nothing, which means *not built*.
Reporting it as "not on PATH" sent the reader hunting for an install.

### 18b - Launch options, not a text box

A single field held the whole command, which covered "change it" and nothing
else: dropping `--port 4000` for an afternoon meant deleting and retyping it.
Now the command is one field, flags are rows that switch off without being
lost (`disabled_args`, remembered), and the line that will really run is
assembled above both.

### 18c - Removing junk sticks

The ten-minute rescan re-added anything whose folder still existed, so
removing the Zig toolchain or a `gitleaks` binary was **futile** -- gone by
click, back by lunch. A removal is now remembered in `settings` under
`scan.dismissed`; adding a folder by hand forgets the removal.

**Deliberately NOT auto-filtered:** six date-stamped folders under `Ops/`
(`Fleet-Activity-20260907`, `Lathe-Personal-OS-20260913`, ...). They look like
generated junk and are not -- they are real project snapshots with real files.
A date-stamp heuristic would have hidden work. They are one click from Archive
if unwanted.

### 18d - The library leads with what is used

Resting order is now pinned -> favourites -> most recently launched -> the 20
never launched, alphabetically. Recency rather than a blended "frecency"
score, because that is one sentence to explain and a number needing a glossary
does not belong on the default surface. Frequency is its own sort ("Most
used"), fed by a new `run_counts` query.

### 18e - The app opens on the library

**Reverses the 2026-08-31 decision** that it opens on the dashboard. The new
argument is Tanner's own: *"I should be able to open it, immediately find what
I need, launch it, and move on"* and *"Avoid excessive dashboards"*. The
dashboard is unchanged and one click away in the sidebar.
`dashboard-verify`'s opening assertion is inverted and says so in place.

### 18f - Files, scripts and folders

`register_project` always accepted a custom command; the Add menu never said
so, so only folders were ever added. It now names what it takes, and choosing
a file proposes the command for its type (`.ps1` -> PowerShell, `.py` ->
python, otherwise the shell) **in an editable field before saving**.

### What the suites caught that eyes did not

- The manual panel and the log panel could be open together, and at 1240px the
  sidebar plus both crushed the library column to a measured **0px** name
  column. They now exclude each other.
- The manual toggle used a JS `matchMedia` subscription while the panel used
  `max-[1099px]:hidden` -- two mechanisms for one rule, which could disagree
  across a resize. Both are the CSS class now.

### Assertions rewritten, and why

Four, each encoding the rule rather than the old value: `a11y-verify` and
`guidelines-verify` matched the Add menu's literal wording; `regress-verify`'s
"fewer than 40 tabbables" was a number picked by hand and is now "fewer tab
stops than the grid has tiles", which is impossible if the grid pays per tile;
and its "toggle is gone" read presence when the toggle became CSS-hidden, so
it reads visibility.

### Not verified from here

Choosing a file opens a native picker CDP cannot drive, so the
extension-to-command proposal is exercised by hand rather than asserted.

## Phase 12 — Project detail ⏳ (the next slice)
Overview · Logs · Performance · Settings · Environment · Dependencies · Build
history · Crash history · Notes.
- Environment values masked by default and scrubbed from logs; `.env` files
  read, never written
- **Settings first**: it unlocks port configuration, which is why the hard
  port-conflict gate cannot currently be reached in practice

## Phase 13 — Organisation & search ⏳
- Command palette (Ctrl+K) — the primary interaction at scale
- Collections, categories, tags · filters that persist across launches
- Global search across name, language, framework, tags, description, path

## Phase 14 — Resident behaviour ⏳
- Tray icon with running count and per-project quick actions
- Start-with-Windows · window state persistence · bulk actions (stop all)

## Phase 15 — Distribution ⏳
- ✅ NSIS installer, Start Menu searchable, no terminal anywhere
- ⏳ [M] Code signing · update channel · crash reporting opt-in

## Phase 16 — Continuous evolution 💭
Cross-machine sync of the registry (files, not a service) · per-project
environment profiles · a runner marketplace (still TOML, still data). Rule:
none of these may require rewriting Phases 1–4.

## Platform
- ✅ Windows 11. `deck-runtime::job` is the **only** platform-specific module
- 💭 [L] POSIX would use process groups and `SIGTERM`/`SIGKILL` behind the same
  interface. Nothing above that module would change

---

## Known open issues
- 🟡 **Benchmarks on a loaded machine are fiction.** A run during heavy
  background builds reported boot at 721 ms with the JS start 4× later than
  normal; the CPU was spiking to 98% from unrelated work. Check the machine is
  idle before believing any timing, and prefer load-independent measures (bundle
  bytes, server-side ms) when it is not.
- 🟡 **A freshly built exe pays a Defender scan on its first run.** One sample
  read 4228 ms against a steady ~570 ms. Always discard the first run after a
  build, or warm the binary before measuring.
- 🟡 **CDP-instrumented timings are not user timings.** Enabling the debugging
  port roughly doubles WebView2 startup. Any boot figure quoted from a CDP run
  is an upper bound on a machine nobody uses; trust `ui_ready` instead.
- 🟡 **The app process has vanished twice** during harness runs. Downgraded from
  🔴: the leading explanation is now the close-kills-everything bug fixed in
  Phase 3b — a stray window close would have exited the process exactly this
  cleanly. **That is a hypothesis, not a diagnosis**, and it has not been
  reproduced since the fix. It exits
  *cleanly* — no WER dump, no Application event-log entry — so something asks it
  to quit rather than it faulting. The leading hypothesis (abrupt CDP disconnect
  with a stale `Emulation` override) was **tested and disproved**. Next step:
  run from a console with stderr captured, and log `RunEvent::Exit` with a reason
- 🟡 **VRAM can read above the card's capacity** (8.1 GB against 7.8 GB
  dedicated). `\GPU Process Memory(*)\Dedicated Usage` is a per-process sum and
  shared surfaces are counted once per process that maps them. The figure is
  labelled "per-process sum" and drawn in the accent rather than the alarm
  colour, because over-100% here is a property of the counter, not of the
  machine — but it is still a number that invites a double-take.
- 🟡 **`cargo build --release` does not produce a shippable binary.** It leaves
  the app pointed at the dev server, so it launches to "localhost refused to
  connect". Use `npx tauri build --no-bundle`. Cost an hour of chasing a view
  that rendered perfectly and verified as empty.
- 🟡 `metrics-verify.mjs` drives the app through `window.__TAURI__`, which
  release builds deliberately do not expose. Dev-build-only by design — do not
  "fix" it by exposing the global; `ui-verify` asserts its absence on purpose

## Verification (must all pass before any milestone is called done)
| Suite | Count | Runs against |
|---|---|---|
| Rust unit + integration | **353** | `cargo test --workspace` |
| Clippy, pedantic | **0 warnings** | `--all-targets` |
| tsc + ESLint | clean | `--max-warnings 0` |
| Sort behaviour | **15** | shipped release build, real mouse input |
| UI / theme / native | **27** | shipped release build |
| Grid, storage, manual, web apps | **27** | `shell-verify`, shipped build |
| Reviewed defects, kept fixed | **21** | `regress-verify`, shipped build |
| Launch readiness, flags, ordering | **28** | `launcher-verify`, real 56-project registry |
| Layout geometry | **34** | 4 widths × panel open/closed |
| Web interface guidelines | **16** | computed styles, shipped build |
| System view | **18** | shipped release build, live counters |
| Stats vs Windows counters | **27** | `tools/verify/stats-accuracy.ps1` |
| Survives window close | **7** | `tools/verify/survives-close.ps1` |
| Boot benchmark | 5 runs | `tools/verify/boot-bench.ps1` |

Browser suites drive the **release** binary over CDP, never a dev server — a
smoke test asserts `location.origin === "http://tauri.localhost"`.

**Every suite can now fail the run.** The eleven suites predating September
2026 always could -- each calls `process.exit(...)` itself. The two added in
Phase 17 did not: they call `finish()` as a statement, and until 2026-09-07
`finish()` set no exit code, so `run-all.ps1` marked them PASS regardless of
what they printed. `finish()` now assigns the code, which closes it for every
future suite as well.

## Review cadence
End of every milestone: code / architecture / UX / accessibility / performance /
durability review. Failures get entries here, not apologies.

**Assertions are part of the design.** When a deliberate change breaks a test,
the test is rewritten to encode the *rule* rather than the old value — and that
rewrite is called out, because a suite edited to stay green proves nothing.
Four have been inverted this way so far: the action-button count (the row now
sheds controls by design), the surface-ladder hexes (pinned values froze the
palette), the seam-darker-than-panel rule (now separators-lighter-than-
surface), and the GPU-engine assertion (it accepted a reading with no named
engine whenever the breakdown was empty, which let a real bug through — every
engine sat under the display threshold, so an idle GPU reported "0.0%, engine
unknown" and looked exactly like broken counters). Each was reported at the
time.
