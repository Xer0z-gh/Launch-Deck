# Launch Deck

A local-first control centre for every project on this machine. Register
a folder — pick it, drop it on the window, or scan a whole workspace —
and Launch Deck works out what it is, how to start it, and then runs,
watches and stops it, whatever language it happens to be written in.

Nothing leaves the machine. No accounts, no cloud, no telemetry, no
network calls of any kind.

Windows-first: Tauri 2 + Rust backend, React frontend, and process
supervision built directly on Windows Job Objects.

---

## Installing

```bash
npm run tauri build
```

produces an NSIS installer at
`target/release/bundle/nsis/Launch Deck_0.1.0_x64-setup.exe`. Run it
once; **Launch Deck** lands in the Start Menu — press Win, type
"launch", Enter. No terminal involved, then or later.

Data lives in `%APPDATA%\Launch Deck\` (database, per-run log files, and
the `runners\` folder for your own plugins).

## What it does today

- **Add projects three ways** — folder picker, drag-and-drop anywhere on
  the window, or a guarded workspace scan that finds everything
  identifiable under a root without walking into `node_modules`,
  `target` or `.git`.
- **Detects what a project is** via 32 bundled runner definitions: Node
  (plus Next.js, Vite, Tauri, Electron, React Native), Python (plus
  Poetry, uv), Rust, Go, Deno, Bun, .NET, Java (Maven and Gradle),
  CMake, Make, PHP (plus Laravel), Ruby (plus Rails), Lua, Dart,
  Flutter, Zig, PowerShell, shell, batch, and bare
  executables. Package managers resolve from lockfiles; versions read
  from the project's own metadata. The add dialog shows the exact
  command Run will execute *before* anything is registered — and for a
  folder nothing recognises, you type the command yourself.
- **Runs and stops the whole tree.** Children spawn suspended into a
  kill-on-close Job Object, then resume — so Stop actually frees the
  port instead of orphaning `npm` → `node` → `vite` → esbuild workers.
  Even a hard kill of Launch Deck itself cannot leave strays behind.
- **Streams logs live** — ANSI colours, stdout/stderr/deck filters,
  search, error emphasis, copy, export, follow mode with a scroll-up
  pause. Thousands of lines a second coalesce into ~16 UI events a
  second; 5,000 lines render as ~40 DOM nodes.
- **Explains failures.** A crash is diagnosed from its own output where
  possible: "port 5173 is already in use", "a dependency is missing —
  try Install", not "exit code 1". Auto-restart (per-project policy)
  backs off exponentially and trips a circuit breaker instead of
  fork-bombing.
- **Dashboard at scale** — table view (default past 30 projects) and
  card view, pinned/favourite/archive, instant search over names,
  languages, frameworks, tags and paths (Ctrl+K), dark/light/system
  theme, keyboard navigation throughout.
- **Every row knows whether it can start** before you press anything —
  a missing folder, an absent toolchain, uninstalled dependencies, an entry
  file that is not there, an ambiguous `cargo run`, or an app that has not
  been built. Each comes with the repair for that cause, and a failure no
  static check can see is carried from run history instead.
- **Launch options in the GUI** — the command in one field, flags as rows you
  can switch off without losing them, and the line that will actually run
  shown above both.
- **Launch anything** — a project folder, a file, a script, a folder to open,
  or a web app. Picking a file proposes the command for its type and lets you
  correct it before saving.
- **The library leads with what you use** — pinned, then favourites, then most
  recently launched. Removing something is remembered, so the background scan
  does not put it back.
- **Two library views** — a grouped list for working, and a dense grid of
  marks for launching. Same projects, same filters, different question.
- **Saved web apps** — register a URL and it lives in the library like
  anything else: searchable, watched, opened in a window inside Launch Deck.
  It is backed by a real Windows `.url` shortcut on disk, not just a row.
- **Storage** — what every project costs on disk, sorted either way, with a
  colour band for absolute size, expandable into subfolders. Junctions are
  listed but never followed, so nothing is counted twice.
- **A manual for each project** — a right-hand pane with what it is, how to
  launch it, what commands it declares and where its docs are. All of it read
  off disk: the README's own first paragraph, the real `package.json` scripts.
  A project with no README says so rather than getting an invented summary.
- **Reads its own apps' status** — a status source can name an app-specific
  reader, so a tile shows Fleet's active runs, Ollama's loaded models, Lathe's
  request count or FocusForge's priority gate instead of only "it answered".
- **Desktop integration** — open a project's folder, a terminal in its
  directory, or VS Code, from the row menu.

## Status

| Layer | State | Tests |
|---|---|---|
| `deck-domain` — types, traits, state machine | done | 58 |
| `deck-runners` — detection, plugins, scan, launch plans | done | 95 |
| `deck-store` — SQLx persistence | done | 18 |
| `deck-runtime` — supervisor, job objects, logs | done | 42 |
| `src-tauri` — IPC adapters + event pumps | done | — |
| Frontend — dashboard, add flows, log viewer | done | — |

213 Rust tests; `cargo clippy --all-targets` clean at
`warn(clippy::pedantic)`; `tsc` strict (with `noUncheckedIndexedAccess`
and `exactOptionalPropertyTypes`) and ESLint clean.

Next up (`docs/ROADMAP.md`): live CPU/RAM/port monitoring, project
detail pages, command palette, tray residency.

## Layout

```
crates/
  deck-domain/    types + traits, zero I/O
  deck-runners/   detection engine, manifest plugins, scan, launch plans
  deck-store/     SQLx repositories, embedded migrations
  deck-runtime/   supervisor, job objects, log pipeline
src-tauri/        Tauri shell — IPC adapters and event pumps only
src/              React frontend (Vite, Tailwind v4, Radix primitives)
runners/          the 32 bundled runner manifests
docs/             ARCHITECTURE.md · RUNNERS.md · ROADMAP.md
```

Dependencies point strictly inward; everything below `src-tauri` is
testable headlessly.

## Adding support for a new language

Write a TOML file — no recompile, no code change:

```toml
[meta]
id = "zig"
name = "Zig"
language = "Zig"

[[detect]]
file_exists = "build.zig"

[commands.run]
exec = { program = "zig", args = ["build", "run"] }
```

Drop it in `%APPDATA%\Launch Deck\runners\`. A manifest sharing an id
with a bundled runner replaces it. Full format — variables, combinators,
health probes, the script escape hatch — in `docs/RUNNERS.md`.

## Developing

```bash
npm run tauri dev
```

```bash
cargo test --workspace
```

```bash
cargo clippy --all-targets
```

Requires Rust 1.85+ (built against 1.94) and Node 20+.

## Two things worth knowing

**Detection never executes anything.** Every detection rule is a
read-only filesystem or parse predicate, rule paths cannot escape the
project root, and file reads are size-capped. Scanning a folder can
never run code from it — commands only ever run from an explicit
action, and they are always a program plus an argument vector, never a
string handed to a shell.

**Stopping kills the whole tree.** `docs/ARCHITECTURE.md` explains the
suspended-spawn → job-assign → resume sequence, why Windows offers no
true graceful signal to a windowed app, and how the two-stage stop
(stdin EOF, then job termination) handles both the polite and the
stubborn cases.
