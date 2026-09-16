# WRITING A RUNNER

======================================================================
WHAT A RUNNER IS
======================================================================

A runner teaches Launch Deck to recognise and drive one kind of project.
It is a TOML file, not code. Drop one in the runners directory and
restart the app — no rebuild, no core change.

    %APPDATA%\Launch Deck\runners\<your-runner>.toml

A user manifest whose `meta.id` matches a bundled runner REPLACES it.
That is how you override a shipped default without editing the install.

The 32 bundled manifests live in `runners/` in the repo and are the best
reference material available. Read `node.toml` for variables and
`powershell.toml` for `first_match`.

======================================================================
MINIMAL EXAMPLE
======================================================================

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

That is a complete, working runner.

======================================================================
THE ONE MISTAKE EVERYBODY MAKES
======================================================================

Write detection rules as `[[detect]]` blocks, NOT as an inline
`detect = [...]` array placed after `[meta]`.

    # WRONG — this is valid TOML that silently means `meta.detect`,
    # leaving the real rule list empty.
    [meta]
    id = "mine"
    detect = [{ file_exists = "package.json" }]

    # RIGHT — order-independent.
    [meta]
    id = "mine"

    [[detect]]
    file_exists = "package.json"

Anything after a `[table]` header belongs to that table. The loader
rejects a manifest with no rules and says so explicitly, so this fails
loudly rather than producing a runner that claims nothing.

======================================================================
[meta] — IDENTITY
======================================================================

    id             REQUIRED. Stable identifier. Letters, digits, hyphen,
                   underscore only — it appears in file paths. Projects
                   in the database reference it, so treat it as
                   permanent once you have used it.
    name           REQUIRED. Shown in the UI, e.g. "Node.js".
    language       REQUIRED. e.g. "TypeScript", "Rust".
    framework      Optional. Set it when the runner identifies one
                   specifically. Cards prefer it over `language`.
    icon           Optional Lucide icon name.
    priority       Default 100. Higher wins when several runners match.
    default_ports  Optional. Used by the pre-launch conflict check.

PRIORITY CONVENTION — follow it and your runner composes correctly with
the bundled ones without either knowing the other exists:

     10    build artefacts (`executable`)
     50    scripts (`batch`)
     60    scripts (`powershell`, `shell`)
     90    weak generic (`make`)
    100    ecosystem generic (`node`, `rust`, `go`, `dotnet`, `python`)
    150    runtime variant (`deno`, `bun`)
    200    framework (`laravel`, `rails`, `flutter`, `python-poetry`)
    250    framework (`vite`)
    300    framework (`nextjs`, `electron`, `react-native`)
    400    meta-framework wrapping others (`tauri`)

Ties break by id, so a project's detected type never changes between
launches.

======================================================================
[[detect]] — PREDICATES
======================================================================

ALL top-level rules must pass. An empty list never matches.

    file_exists = "package.json"
    dir_exists  = "src-tauri"
    glob_matches = "*.csproj"          # project root only, not recursive

    json_key_exists = { file = "package.json", pointer = "scripts/dev" }
    json_key_equals = { file = "package.json", pointer = "type",
                        value = "module" }
    toml_key_exists = { file = "pyproject.toml", path = "tool.poetry" }
    file_contains   = { file = "Makefile", text = "gcc" }

Combinators nest freely:

    [[detect]]
    any_of = [
      { file_exists = "vite.config.ts" },
      { file_exists = "vite.config.js" },
    ]

    [[detect]]
    not = { file_exists = "Pipfile" }

    [[detect]]
    all_of = [ ... ]

NOTES

• JSON pointers are RFC 6901; a leading `/` is optional, so
  `scripts/dev` and `/scripts/dev` are the same.
• `json_key_equals` coerces numbers and booleans to strings, so you can
  compare against `"true"` or `"2"`.
• `glob_matches` supports `*` and `?` over one filename, case
  insensitively. `.` is a literal dot.
• `file_contains` reads at most the first 256 KiB.
• Paths cannot escape the project root. `../../../Windows` matches
  nothing.
• A malformed project file is a NON-MATCH, never an error — one broken
  `package.json` must not abort a whole workspace scan.

THERE IS DELIBERATELY NO RULE THAT EXECUTES ANYTHING. Detection runs
automatically over directories the user merely pointed at; a manifest
able to run commands during detection would turn "scan this folder" into
"run whatever this folder says".

======================================================================
[[vars]] — VARIABLES FOR COMMAND TEMPLATES
======================================================================

Resolution order: `cases` (first match wins) → `from_value` →
`first_match` → `default`.

```toml
[[vars]]
name = "pm"
default = "npm"
cases = [
  { value = "pnpm", when = { file_exists = "pnpm-lock.yaml" } },
  { value = "yarn", when = { file_exists = "yarn.lock" } },
  { value = "bun",  when = { file_exists = "bun.lockb" } },
]
```

`cases` are ordered, so a lockfile priority list reads top to bottom
exactly as written.

`from_value` pulls a value out of project metadata:

```toml
[[vars]]
name = "entrypoint"
default = "index.js"
from_value = { json = { file = "package.json", pointer = "main" } }
```

`first_match` uses the alphabetically first root file matching a glob.
This is what makes script and executable runners possible — a folder
containing `deploy.ps1` needs a command naming THAT file, which no fixed
template can express:

```toml
[[vars]]
name = "entry"
default = "main.ps1"
first_match = "*.ps1"
```

Matches are sorted before the first is taken, so a folder with both
`build.ps1` and `deploy.ps1` resolves identically every launch.

ALWAYS AVAILABLE, no declaration needed:

    {projectRoot}   absolute path to the project
    {projectName}   the project's display name
    {dirName}       the directory's own name

======================================================================
[commands] — LIFECYCLE STEPS
======================================================================

`run` is REQUIRED. Everything else is optional, and absence is how you
say "this does not apply" — a shell script has nothing to install, and
that is expressed by omission rather than by a stub that does nothing.

    install  build  run  debug  clean  test

```toml
[commands.run]
exec = { program = "{pm}", args = ["run", "{script}"] }

[commands.build]
exec = { program = "{pm}", args = ["run", "build"] }
```

A command is ALWAYS a program plus an argument vector. There is no
shell-string form, on purpose: nothing is handed to `cmd /c` for
re-parsing, so a path containing spaces or an `&` cannot become two
commands.

Templates support `{var}` substitution only — no conditionals, no loops.
`{{` and `}}` are literal braces. A `{name}` with no matching variable
is an ERROR, not an empty string: silently turning `npm run {script}`
into `npm run` would start the wrong thing.

WHEN A TEMPLATE IS NOT ENOUGH, delegate to a script. This is the
intended escape hatch, and it is explicit rather than hidden in clever
syntax:

```toml
[commands.run]
script = { path = "start.ps1", args = ["{projectRoot}"] }
```

Script paths resolve relative to the manifest's own directory, so you can
ship a helper alongside your `.toml`. The interpreter is chosen by
extension — `.ps1` runs via `powershell.exe -NoProfile -File`, `.sh` via
bash, `.py` via python, `.bat`/`.cmd` via `cmd /c`.

`run` and `debug` are treated as LONG-RUNNING: a clean exit means the
server stopped, not that it succeeded. `install`, `build`, `test` and
`clean` are tasks, where exit code 0 is success.

======================================================================
[version] AND [health]
======================================================================

```toml
[version]
json = { file = "package.json", pointer = "version" }
# or
toml = { file = "Cargo.toml", path = "package.version" }
```

```toml
[health]
log_contains = { text = "ready in" }
# or
http = { url = "http://localhost:{port}/", expect_status = [200] }
# or
tcp_port = { port = "{port}" }
```

A health probe distinguishes "process alive" from "actually serving". A
dev server exists for several seconds before it answers, and without a
probe "open in browser" fires too early.

======================================================================
VALIDATION — WHAT GETS REJECTED AT LOAD
======================================================================

• empty or missing `meta.id`
• an `id` containing characters unsafe in a path
• no detection rules (see THE ONE MISTAKE, above)
• no `[commands.run]`
• a `[[vars]]` entry with an empty name

One bad manifest is skipped and reported — it never stops the app from
starting or prevents other runners from loading.

======================================================================
CHECKLIST BEFORE YOU SHIP ONE
======================================================================

□ `[[detect]]` blocks, not an inline array after `[meta]`
□ Detection is specific enough not to claim unrelated directories
□ Priority sits correctly relative to the table above
□ `run` exists and starts the thing a person would expect
□ Every `{var}` used is declared or built in
□ Tested against a real project of that type, and against one that
  should NOT match
