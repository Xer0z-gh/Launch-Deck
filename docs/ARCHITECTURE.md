# LAUNCH DECK — ARCHITECTURE

======================================================================
THE OBJECTIVE
======================================================================

One local-first control centre for every project on this machine,
regardless of language or framework. Register it, identify it, launch
it, watch it, stop it, read its output. Nothing leaves the machine: no
accounts, no telemetry, no network calls of any kind.

The bar is a tool that manages hundreds of projects across any language
and is still pleasant on day four hundred. That rules out designs that
work at ten projects and collapse at three hundred.

======================================================================
THE GATES (verify before calling ANY task complete)
======================================================================

• Would this survive a professional code review?
• Would this survive a professional design review?
• Would users immediately understand how to use it?
• Would professionals trust it in commercial work?
• Would someone willingly pay for this product?
• Would this strengthen the brand?

If any answer is "no," continue refining.

======================================================================
CRATE LAYOUT — DEPENDENCIES POINT INWARD, ALWAYS
======================================================================

    deck-domain      types + traits. Zero I/O. Depends on nothing.
        ^
        |----------- deck-runners    detection + plugin manifests
        |----------- deck-store      SQLx repositories + migrations
        |----------- deck-runtime    supervisor, job objects, logs
                          ^
                          |
                     src-tauri       IPC adapters ONLY

Rules that are not negotiable:

• `deck-domain` never gains a dependency that touches the outside
  world. If a change would require one, the change belongs elsewhere.
• `src-tauri` contains no logic. A Tauri command validates its input,
  calls one method, maps the error. If a command grows a branch, the
  branch belongs in a crate.
• Every crate below `src-tauri` is testable headlessly. The supervisor
  tests spawn real processes and assert on real exits with no GUI and
  no Tauri runtime present.

Deliverable: `cargo test --workspace` passes without a display.

======================================================================
WHY THESE TECHNOLOGIES
======================================================================

Tauri 2 over Electron
    A resident control centre must not itself be the heaviest thing
    running. Tauri uses the OS webview instead of bundling Chromium:
    tens of megabytes rather than hundreds, and no second browser
    engine competing with the projects being supervised.

Rust for the backend
    Process supervision is systems work — raw Win32 handles, job
    objects, pipe plumbing. Rust gives direct access to that with
    ownership rules that make handle lifetimes explicit. The `unsafe`
    footprint is confined to `deck-runtime::job` and every block there
    carries a SAFETY comment.

SQLx, NOT Prisma
    Prisma was specified and rejected. It is a Node/TypeScript ORM,
    while all data access here lives in Rust; adopting it would mean
    either a Node sidecar process purely to reach SQLite, or moving
    persistence into the frontend and abandoning the layering above.
    It also ships a per-platform query-engine binary to bundle.

    SQLx is Rust-native and needs no extra process. Queries are
    runtime-bound rather than macro-checked: the `sqlx::query!` macros
    require the `sqlx-cli` toolchain and a re-prepared offline cache on
    every schema change, and buy nothing the repository tests do not
    already prove — every query runs against a real temp database in
    `cargo test`, so a typo'd column fails the suite instead of the
    build. Same gate in CI, one less toolchain dependency.

React + TypeScript strict + Vite
    Dense, stateful, live-updating UI. `tsconfig` runs strict with
    `noUncheckedIndexedAccess` and `exactOptionalPropertyTypes`.

TanStack Query + Zustand
    Query owns server state (projects, history, detection results) with
    its cache and invalidation. Zustand owns view state (filters, sort,
    selection, which panel is open). Keeping the two separate is what
    stops "is this loading?" and "is this filtered?" from tangling.

Radix primitives, NOT the shadcn CLI
    Deviation from the brief, deliberately. shadcn/ui is Radix plus
    Tailwind plus a default visual style — and that default style is
    the single most recognisable "AI-generated app" look. Radix is used
    directly for the accessibility-critical primitives (dialog, menu,
    tooltip, tabs, scroll area) and styled from our own tokens, which
    yields identical keyboard and screen-reader behaviour with an
    identity that is ours.

sysinfo / tracing / Lucide
    As specified. Recharts was in the original stack for detail-page
    charts; it was installed but never imported, and was removed on
    2026-07-30 along with six other zero-import dependencies. Add a
    charting library back when Slice 3 needs one.

    The per-row sparklines stay hand-rolled SVG regardless: a charting
    library rebuilds its SVG tree on every data change, and at 1 Hz
    across twenty visible rows that is visible stutter for what amounts
    to one `<polyline>`.

======================================================================
THE PLUGIN MODEL — A RUNNER IS DATA, NOT CODE
======================================================================

Requirement: "the core application should never need modification to
support new languages." A Rust trait alone cannot satisfy that — adding
a language would mean editing and recompiling the core.

So a runner is a TOML file. It declares:

    [meta]      identity, language, framework, icon, priority, ports
    [[detect]]  read-only predicates that must all pass
    [[vars]]    variables resolved from the directory
    [commands]  a command template per lifecycle step
    [version]   where to read the project's own version
    [health]    optional readiness probe

`RunnerRegistry` loads the 32 bundled manifests compiled into the
binary, then overlays user manifests from the app data directory. A user
file replaces a bundled one with the same id. Adding Zig support is
dropping in `zig.toml`. No recompile, no core change.

`deck_domain::runner::Runner` remains the internal interface and
`ManifestRunner` is the single implementation behind it. If an ecosystem
ever proves genuinely undescribable declaratively, a second
implementation slots in behind the same trait — the extension point
exists without paying for it now.

TWO PROPERTIES THAT CARRY THEIR WEIGHT:

Detection cannot execute anything.
    Detection runs automatically over directories the user merely
    pointed at. A manifest able to run a command during detection would
    turn "scan this folder" into "run whatever this folder says". Every
    rule is a filesystem or parse predicate; rule paths cannot escape
    the project root; file reads are capped at 256 KiB. Execution
    happens only from an explicit user action.

Identity is separate from configuration.
    A framework is its own manifest at a higher priority — Next.js at
    300 outranks generic Node at 100 without either knowing the other
    exists. A package manager is a *variable* resolved from lockfiles,
    so one `node.toml` covers npm, pnpm, yarn and bun instead of four
    near-identical files.

Deliverable: `docs/RUNNERS.md` documents the format; a test asserts
every bundled manifest parses and validates, so a broken shipped runner
cannot reach a user.

======================================================================
PROCESS SUPERVISION — THE PART MOST TOOLS GET WRONG
======================================================================

`npm run dev` is not one process. It is npm, which starts node, which
starts Vite, which starts esbuild workers. Killing the pid you spawned
leaves every descendant alive, still holding the port, invisible, and
orphaned until the user finds it in Task Manager.

THE SPAWN SEQUENCE IS FIXED:

    1. Create a Windows Job Object
       (KILL_ON_JOB_CLOSE | BREAKAWAY_OK | DIE_ON_UNHANDLED_EXCEPTION)
    2. Spawn the child SUSPENDED
    3. Assign the child to the job
    4. Resume the child's main thread

Steps 2–4 close the window in which a child could spawn a grandchild
that escapes the job permanently. Resuming requires locating the main
thread by ToolHelp snapshot, because `std::process::Child` does not
expose it. That is the price of closing the race, and it is worth it:
the alternative is the orphaned-process bug this design exists to
prevent.

KILL_ON_JOB_CLOSE means even a hard kill of Launch Deck itself takes
the supervised trees down with it.

STOPPING IS TWO STAGES, because Windows has no SIGTERM:

    1. Close the child's stdin. Many CLI tools read EOF as "shut down"
       and get to release ports and flush output cleanly.
    2. After the grace period, terminate the job — the whole tree,
       atomically.

`GenerateConsoleCtrlEvent` is the nearest thing to a signal and requires
sharing a console with the target, which a windowed Tauri app does not
have. This is a platform limitation stated plainly, not a shortcut.
`RunState::Stopping` models the interval so the UI shows progress rather
than flickering between running and stopped.

CRASH-LOOP PROTECTION: auto-restart without backoff turns a project
that crashes on startup into a fork bomb. Restarts use exponential
backoff (500 ms doubling, 30 s ceiling) and stop at the policy's attempt
ceiling. A successful run resets the counter.

EXIT DIAGNOSIS: "exit code 1" tells the user nothing. The supervisor
reads the log tail and names the likely cause — port already in use
(with the port number), missing dependency, command not on PATH,
permission denied, out of memory. It only ever ADDS an explanation; it
never changes an exit status or hides a line.

Deliverable: 42 tests spawning real processes, including one that
verifies a `ping.exe` grandchild dies with its `cmd.exe` parent.

======================================================================
THE LOG PIPELINE — VOLUME IS THE DESIGN PROBLEM
======================================================================

A watch-mode build emits megabytes a minute; a crash loop emits the same
stack trace forever. Three decisions follow:

• Lines live in FILES, not the database. SQLite holds a path and run
  metadata. A schema storing individual log lines would grow without
  bound and make every query slow.
• Memory is a BOUNDED RING — 5,000 lines per run, with a dropped count
  so the viewer can say "1,204 earlier lines" instead of silently lying.
• Broadcast is LOSSY ON PURPOSE. A subscriber that cannot keep up misses
  lines rather than applying backpressure. Blocking a build because a
  log viewer is slow would be the wrong trade every time.

ANSI escapes are stored RAW. Stripping happens at search and export
time; once discarded the colour is gone.

Sequence numbers are per-run and monotonic across both streams.
Timestamps are insufficient — at this volume many lines share a
millisecond and ordering must be exact.

IPC coalescing lives at the Tauri boundary, not in the pipeline: one
event per line saturates the bridge, so the consumer batches on a ~60 ms
flush.

======================================================================
SCANNING — GUARDS, NOT A NAIVE WALK
======================================================================

An unguarded recursive walk of a workspace is unusable, not merely slow.
One `node_modules` holds tens of thousands of directories; a Rust
`target/` holds more; a `.git` object store more again.

• Depth cap (default 4). Projects live near the top of a tree.
• Ignore list — 40 entries covering dependency stores, build output, VCS
  metadata, virtualenvs, editor state. Dotted directories are skipped
  wholesale.
• STOP ON MATCH. Once a directory is identified, its children are not
  scanned. Without this a Cargo workspace reports every member crate and
  a Tauri app reports itself plus its `src-tauri` — technically true,
  useless in a list. `prune_nested: false` opts out for monorepos.
• Result cap, with a `truncated` flag so a partial result never looks
  complete.

Breadth-first, children sorted, so output is stable across runs and the
cap truncates the deep tail rather than an arbitrary branch.

======================================================================
UI ARCHITECTURE — DENSITY THAT SCALES
======================================================================

The brief asked for twelve data points and ten action buttons per card.
At a hundred projects that is 1,200 data points and 1,000 click targets
on one screen. Unequal information must not get equal visual weight.

SO:

• Cards show status, name, kind, and live metrics ONLY while running.
  Description, version and last-launched are metadata — they live on the
  detail page and in the hover state.
• A TABLE VIEW is the default above ~30 projects. Docker Desktop and PM2
  both use tables at scale for exactly this reason. Cards do not survive
  three hundred rows.
• ONE contextual primary action (Run ↔ Stop, never both — enforced by a
  test over the state machine), one secondary (Restart), the rest in an
  overflow menu.
• A COMMAND PALETTE (Ctrl+K) is a first-class feature, not a nicety.
  It is how anyone actually drives hundreds of items, and it is the
  Raycast half of the reference set.

Hierarchy comes from weight, opacity, size, and colour — in that order.
Colour is never the primary mechanism. Changing numbers use tabular
figures so digits do not reflow.

Accent means one thing: LIVE. Running processes, selected rows, focused
controls. It is never decoration. Surfaces are three steps; depth comes
from hierarchy and borders, not shadows.

The palette does not max out contrast. Near-black under pure white
measures ~17:1, past the point where more helps and the main cause of
strain over an hour at a dashboard. Charcoal under soft white sits near
13:1 — still well clear of AAA, materially calmer.

======================================================================
======================================================================
LAUNCHING ON WINDOWS -- THE .cmd PROBLEM
======================================================================

`npm`, `pnpm`, `yarn`, `tsc`, `gradlew` and `mvnw` are not executables
on Windows. They are `.cmd` or `.bat` shims. `CreateProcess` -- and so
`std::process::Command` -- resolves only directly-executable images from
`PATH`, and a batch file cannot be executed directly even given its full
path, because it has no PE header.

The effect was that EVERY Node-based project failed to launch with a
misleading "`npm` was not found on PATH", on a machine where npm works in
every shell. That is most of a typical project library.

`deck-runtime::program` resolves a program the way a shell does: search
`PATH` honouring `PATHEXT` order (so a directory holding both `foo.exe`
and `foo.cmd` resolves to the exe, as a shell would), then report whether
the result is directly executable or must go through `cmd.exe /c`. The
supervisor routes accordingly.

Worth noting how long this survived: every earlier end-to-end test used
`cmd.exe` and `python`, both real `.exe` files, so the entire npm surface
was never exercised. It surfaced only when a test finally launched a real
project. A test now runs an actual `npm run` script and asserts on its
output.

======================================================================
SINGLE INSTANCE MUST GATE THE DATABASE
======================================================================

Registration order in `run()` is load-bearing. The single-instance
plugin decides during `Builder::build()`, and a losing instance exits
there -- so anything that opens the database must happen AFTER that
point, which means inside `setup()`.

Getting this wrong produced a real failure. `Store::open` used to run
before the builder, so a second launch would:

  * create its own SQLite connection pool,
  * run migrations,
  * evaluate the self-heal branch,
  * write a `VACUUM INTO` snapshot,

and only then be exited by the plugin. Two processes contending for one
SQLite file surfaces as `SQLITE_BUSY` past the busy timeout on whichever
loses, which the UI showed as "Could not load projects -- the database
did not answer".

The store is now created in `setup()`, so a duplicate launch never
touches the database. Verified by the snapshot log: a second launch
against a running instance writes no snapshot, and the snapshot count
does not move. The exit handler uses `try_state`, because an instance
that lost the race never managed any state and panicking on the way out
would turn a clean exit into a crash.

TWO SUPPORTING FIXES, both about not destroying evidence:

  * The error surface used `error instanceof Error ? error.message : ...`.
    A rejected Tauri command yields a plain `{code, message}` object, so
    that check was always false and the real message was discarded --
    which is why the original failure arrived with no diagnosis at all.
    It now goes through `asIpcError`.
  * Queries retry twice, with backoff, when the backend marks an error
    `retryable`. A briefly-locked local database should be a hiccup, not
    an error screen the user has to dismiss.

DURABILITY — WHY THE REGISTRY CANNOT BE LOST
======================================================================

A registry the user curated by hand is expensive to rebuild and cheap
to copy. This section exists because a real one was lost, and the
default SQLite settings were the reason.

THE ROOT CAUSE, MEASURED

`synchronous = NORMAL` in WAL mode does not fsync on commit -- it
fsyncs at checkpoints -- and `wal_autocheckpoint` defaults to 1000
pages. A registry of a few dozen rows never reaches 1000 pages, so the
observed state was a 45 KB database file beside a 362 KB write-ahead
log: essentially all the data lived in a side file that had never been
folded in. Anything that discarded that WAL reverted the registry to
its last checkpoint, which was almost empty.

FOUR LAYERS, INNERMOST FIRST

1. `synchronous = FULL` -- every commit is fsynced, so a committed
   project survives the process being killed outright.
2. `wal_autocheckpoint = 4` pages -- the database file itself stays
   continuously current, which also makes a plain copy of `deck.db` a
   usable backup, and a `wal_checkpoint(TRUNCATE)` on app exit.
   Verified: after a write the WAL is 0 bytes and the data is in the
   file. A test copies ONLY `deck.db` -- no `-wal`, no `-shm` -- and
   asserts the rows are still there.
3. Rotating snapshots -- every open of a populated database writes a
   `VACUUM INTO` snapshot to `backups/`, three kept. `VACUUM INTO`
   rather than a file copy because it is transactionally consistent
   even with a live WAL. An EMPTY database is never snapshotted, or the
   good snapshots would rotate away exactly when they are needed.
4. Self-heal -- an empty registry sitting beside a populated snapshot
   means data was lost, so the next open restores it automatically and
   logs loudly. Verified against the real 27-project registry: deleted
   every row, launched once, all 27 came back.

RESTORE COPIES ROWS, NOT FILES

The first implementation closed the pool, deleted the side files,
overwrote `deck.db` and reopened. On Windows that races the OS
releasing the handles: measured at roughly two successes in three. A
recovery path that works two times in three is worse than none,
because it fails precisely when it is needed. The restore now uses
`ATTACH DATABASE` and copies rows inside one transaction -- no file
surgery, no pool close, deterministic. Five consecutive runs, five
passes.

SCAN ROOTS ARE THE SEED

Every scanned workspace root is persisted. Given the roots, the whole
library can be re-derived, so the registry is reconstructible rather
than irreplaceable. The sidebar shows them with a rebuild action that
re-scans and adds anything missing -- it only ever adds, never
removes, so it is safe to press at any time.

======================================================================
BUILDING — AND ONE FOOTGUN
======================================================================

Always build through the Tauri CLI:

    npm run tauri build     # release exe + NSIS installer
    npm run tauri dev       # dev, Vite on localhost:1420

`cargo build --release` alone produces a binary that still points at
`devUrl` (`http://localhost:1420`) and therefore shows nothing without a
Vite server running. The CLI is what builds the frontend and hands the
compiled assets to `tauri-build` for embedding. Verified the hard way:
a cargo-built binary reported `location.origin ===
"http://localhost:1420"`, while the CLI-built one reports
`http://tauri.localhost`.

`http://tauri.localhost` is NOT a network address. It is Tauri's
in-process custom-scheme handler serving the assets compiled into the
exe -- the same role `res://` plays for a classic Win32 app. The native
audit confirms the running process holds zero listening sockets and
makes zero outbound connections.

The release smoke test asserts the origin, so a dev-URL binary cannot
be shipped unnoticed.

======================================================================
SECURITY AND PRIVACY POSTURE
======================================================================

• No network calls. Not for updates, not for telemetry, not for icons.
• Detection never executes. See the plugin section.
• Commands are ALWAYS a program plus an argument vector — never a shell
  string. Nothing is handed to `cmd /c` for re-parsing, so a directory
  named `My Project & Co` cannot become two commands and a manifest
  cannot smuggle in a shell operator.
• Manifest paths cannot escape the project root.
• Environment values whose names look credential-shaped (SECRET, TOKEN,
  PASSWORD, API_KEY, …) default to masked and are scrubbed from logs.
  A false positive costs one click; a false negative writes a token to
  disk in plain text.
• `.env` files are read, never written.

======================================================================
STANDING ORDER
======================================================================

Nothing ships unfinished. Nothing requires an explanation. If a redesign
produces a better long-term product, choose the redesign.
