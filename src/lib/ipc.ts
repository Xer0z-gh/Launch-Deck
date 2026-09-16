/**
 * The typed IPC surface: every command and event Launch Deck's backend exposes.
 *
 * These types mirror the Rust DTOs in `src-tauri/src/dto.rs` field for field.
 * If a shape changes there, it changes here in the same commit — the two files
 * are one contract split across two languages.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// ---- Errors -----------------------------------------------------------------

/** The error shape every command can reject with. */
export interface IpcError {
  /** Stable discriminant, e.g. `port_in_use` — switch on this, never on text. */
  code: string;
  message: string;
  retryable: boolean;
}

/** Narrow an unknown rejection into an IpcError. */
export function asIpcError(e: unknown): IpcError {
  if (
    typeof e === "object" &&
    e !== null &&
    "code" in e &&
    "message" in e &&
    typeof (e as { code: unknown }).code === "string"
  ) {
    return e as IpcError;
  }
  return {
    code: "unknown",
    message: typeof e === "string" ? e : "Something went wrong",
    retryable: false,
  };
}

// ---- Wire types ---------------------------------------------------------------

export interface ProjectDto {
  id: string;
  name: string;
  description: string | null;
  root: string;
  runnerId: string;
  language: string;
  framework: string | null;
  kindLabel: string;
  packageManager: string | null;
  version: string | null;
  tags: string[];
  category: string | null;
  favorite: boolean;
  pinned: boolean;
  archived: boolean;
  notes: string | null;
  createdAt: string;
  updatedAt: string;
  lastLaunchedAt: string | null;
  runCommand: string | null;
  supported: string[];
  /** Lucide glyph name from the runner manifest, for the fallback tile. */
  glyph: string | null;
  /** How many times this has ever been launched from here. */
  launchCount: number;
  /** Extra arguments appended to the run command, in order. */
  args: string[];
  /** Arguments switched off but remembered, so a toggle is not a retype. */
  disabledArgs: string[];
  /** The project's status source, when one is wired. */
  surface: SurfaceConfigDto | null;
}

/** A status source: what this tile watches beyond the supervisor. */
export interface SurfaceConfigDto {
  /** Pinged for reachability and opened by the tile's open action. */
  url: string | null;
  /** Local TCP port probed for "running even though we did not start it". */
  probePort: number | null;
  /** Log file watched by modification time. */
  externalLog: string | null;
  /** App-specific reader that turns this source into glanceable facts. */
  facts: FactsSource | null;
}

/** Kebab-cased discriminant of the app-specific fact readers the backend supports. */
export type FactsSource =
  | "fleet"
  | "ollama"
  | "lathe"
  | "focus-forge"
  | "pavlok-status";

/** One reader-produced fact, already formatted for display. */
export interface SurfaceFactDto {
  label: string;
  value: string;
}

/**
 * One reading of a status source. Unconfigured probes are `null` end to end;
 * the chip renders nothing for them rather than a fabricated value.
 */
export interface SurfaceStatusDto {
  portOpen: boolean | null;
  httpStatus: number | null;
  httpMs: number | null;
  httpError: string | null;
  /** Response media type (parameters stripped). Only `text/html` should be
   * offered as an embed target -- an API endpoint must not be framed as a page. */
  contentType: string | null;
  logMtime: string | null;
  /** Facts from the configured reader, empty when none configured or nothing to say. */
  facts: SurfaceFactDto[];
  checkedAt: string;
}

/** A project's context card: LAUNCHDECK.md at the repo root, verbatim. */
export interface ContextCardDto {
  path: string;
  exists: boolean;
  content: string;
}

/**
 * Recent lines from one log source. `matched: false` means nothing carried an
 * error mark and the lines are a plain raw tail -- the UI and the generated
 * prompt must label that case as a tail, never as "errors".
 */
export interface ErrorTailDto {
  source: string;
  lines: string[];
  matched: boolean;
}

/** One entry of run history; `projectId` keys cross-project consumers. */
export interface RunRecordDto {
  runId: string;
  projectId: string;
  lifecycle: string;
  command: string;
  startedAt: string;
  finishedAt: string | null;
  exitCode: number | null;
  outcome: string;
  durationMs: number | null;
}

export type RunTag =
  | "idle"
  | "starting"
  | "running"
  | "stopping"
  | "exited"
  | "crashed"
  | "restarting";

export interface RunStateDto {
  tag: RunTag;
  pid?: number;
  startedAt?: string;
  /**
   * Readiness, three-valued on purpose.
   *
   * `true` serving · `false` up but not yet bound (renders as "Starting") ·
   * `null` nothing to wait for, e.g. a script or a build. Serde sends `null`
   * rather than omitting the key, so this is nullable and NOT optional --
   * typing it `boolean?` quietly excluded the case the backend actually emits.
   */
  healthy?: boolean | null;
  code?: number;
  at?: string;
  reason?: string;
  attempt?: number;
}

export interface MetricsDto {
  projectId: string;
  pid: number;
  at: string;
  /** Percentage of ONE core: a four-core build reads ~400, not clamped to 100. */
  cpuPercent: number;
  memoryBytes: number;
  processCount: number;
  threadCount: number;
  listeningPorts: number[];
  uptimeSecs: number;
}

export interface MetricsEvent {
  samples: MetricsDto[];
}

export interface LogLineDto {
  seq: number;
  stream: "stdout" | "stderr" | "deck";
  text: string;
  at: string;
}

export interface LogsDto {
  lines: LogLineDto[];
  dropped: number;
  total: number;
}

export interface DetectionDto {
  runnerId: string;
  runnerName: string;
  language: string;
  framework: string | null;
  version: string | null;
  proposedRun: string | null;
}

export interface InspectDto {
  name: string;
  root: string;
  matches: DetectionDto[];
  alreadyRegistered: string | null;
}

export interface ScanHitDto {
  name: string;
  root: string;
  runnerId: string | null;
  kindLabel: string | null;
  proposedRun: string | null;
  alreadyRegistered: boolean;
}

export interface ScanResultDto {
  hits: ScanHitDto[];
  directoriesVisited: number;
  truncated: boolean;
}

export interface ProjectPatch {
  name?: string;
  description?: string | null;
  favorite?: boolean;
  pinned?: boolean;
  archived?: boolean;
  tags?: string[];
  category?: string | null;
  notes?: string | null;
  runCommand?: string | null;
  /** `null` clears the status source; an object replaces it. */
  surface?: SurfaceConfigDto | null;
  /** A new folder for a project that moved. Re-detects the runner. */
  root?: string;
  /** Replaces the extra-argument list wholesale. */
  args?: string[];
  /** Replaces the switched-off argument list wholesale. */
  disabledArgs?: string[];
}

// ---- Events ---------------------------------------------------------------------

export interface StateEvent {
  projectId: string;
  runId: string;
  state: RunStateDto;
}

export interface LogsEvent {
  projectId: string;
  lines: LogLineDto[];
}

export interface RunFinishedEvent {
  projectId: string;
  runId: string;
  outcome: string;
  exitCode: number | null;
}

export function onStateEvent(handler: (e: StateEvent) => void): Promise<UnlistenFn> {
  return listen<StateEvent>("deck://state", (event) => handler(event.payload));
}

export function onLogsEvent(handler: (e: LogsEvent) => void): Promise<UnlistenFn> {
  return listen<LogsEvent>("deck://logs", (event) => handler(event.payload));
}

export function onMetricsEvent(
  handler: (e: MetricsEvent) => void,
): Promise<UnlistenFn> {
  return listen<MetricsEvent>("deck://metrics", (event) => handler(event.payload));
}

export function onRunFinished(
  handler: (e: RunFinishedEvent) => void,
): Promise<UnlistenFn> {
  return listen<RunFinishedEvent>("deck://run-finished", (event) =>
    handler(event.payload),
  );
}

// ---- Commands ---------------------------------------------------------------------


// ---- Diagnostics ---------------------------------------------------------

/** Worst-first severity for a diagnostic check. */
export type CheckSeverity = "fail" | "warn" | "ok" | "info";

/** One evaluated statement about the app's health. */
export interface CheckDto {
  id: string;
  section: string;
  label: string;
  severity: CheckSeverity;
  detail: string;
}

/** A labelled fact in the raw report. */
export interface FactDto {
  label: string;
  value: string;
}

/** A group of related facts. */
export interface DiagnosticsSectionDto {
  title: string;
  facts: FactDto[];
}

/** The full developer diagnostics report. */
export interface DiagnosticsDto {
  generatedAt: string;
  /** Already sorted worst-first by the backend. */
  checks: CheckDto[];
  sections: DiagnosticsSectionDto[];
}

/**
 * Whether a project's dependencies are installed.
 *
 * Exists because a project with no `node_modules` used to surface as a CRASH
 * several seconds after Run -- indistinguishable from the launcher breaking it.
 * The information was on disk the whole time.
 */
export interface SetupReportDto {
  projectId: string;
  needsSetup: boolean;
  hint: string | null;
  missing: string | null;
  /** The exact command that fixes it, resolved with the project's real package manager. */
  fixCommand: string | null;
  /** False when the folder is gone, which no install fixes. */
  rootExists: boolean;

  /**
   * What stops this project launching, if anything. `null` means Run should
   * work. A closed set, because each value maps to one repair the UI offers.
   */
  blocker: "root" | "setup" | "program" | "entry" | "ambiguous" | "unbuilt" | null;
  /** One sentence naming exactly what is wrong. */
  reason: string | null;
  /** For `"ambiguous"`, the binaries to choose between. */
  choices: string[];
  /**
   * Exit code of the most recent run, when that run failed.
   *
   * Evidence, not prediction: a module import that fails at runtime cannot be
   * seen by any filesystem probe, so the last outcome stands in. Always
   * labelled "last run failed", never "this will fail".
   */
  lastFailedExit: number | null;
}

export const api = {
  listProjects: () => invoke<ProjectDto[]>("list_projects"),
  runStates: () => invoke<StateEvent[]>("run_states"),
  getLogs: (id: string, after?: number) =>
    invoke<LogsDto>("get_logs", { id, after }),

  inspectPath: (path: string) => invoke<InspectDto>("inspect_path", { path }),
  scanPath: (path: string, pruneNested = true) =>
    invoke<ScanResultDto>("scan_path", { path, pruneNested }),

  registerProject: (args: {
    path: string;
    name?: string;
    runnerId?: string;
    customCommand?: string;
  }) => invoke<ProjectDto>("register_project", args),
  updateProject: (id: string, patch: ProjectPatch) =>
    invoke<ProjectDto>("update_project", { id, patch }),
  removeProject: (id: string) => invoke<void>("remove_project", { id }),

  startProject: (id: string, lifecycle?: string) =>
    invoke<string>("start_project", { id, lifecycle }),
  stopProject: (id: string, force = false) =>
    invoke<void>("stop_project", { id, force }),
  restartProject: (id: string) => invoke<string>("restart_project", { id }),

  openFolder: (id: string) => invoke<void>("open_project_folder", { id }),
  diagnostics: () => invoke<DiagnosticsDto>("diagnostics"),
  setupReports: () => invoke<SetupReportDto[]>("setup_reports"),
  uiReady: (marks: {
    domInteractiveMs: number;
    scriptEvalMs: number;
    reactMountMs: number;
    fetchStartMs: number;
    dataArrivedMs: number;
    paintedMs: number;
  }) => invoke<void>("ui_ready", { marks }),
  openTerminal: (id: string) => invoke<void>("open_project_terminal", { id }),
  openInEditor: (id: string) => invoke<void>("open_in_editor", { id }),
  exportLogs: (id: string, dest: string) => invoke<void>("export_logs", { id, dest }),

  projectMetrics: (id: string) => invoke<MetricsDto[]>("project_metrics", { id }),
  scanRoots: () => invoke<string[]>("scan_roots"),
  rescanRoots: () => invoke<number>("rescan_roots"),

  /** Every icon in one call. See `ProjectIcon` for why the per-id form is not used. */
  projectIcons: () => invoke<Record<string, string | null>>("project_icons"),
  /**
   * Asks the backend to read a project's program into the OS file cache.
   *
   * Fire-and-forget, on hover and selection. Launching a binary that had not
   * run recently cost 1.4-1.8 s against 4-85 ms for the same binary a second
   * time; priming ahead of the click took a 630 ms median down to 149 ms.
   * Never awaited and never surfaced -- a failed prewarm costs nothing but the
   * speed-up, and the launch itself reports real problems properly.
   */
  prewarmProject: (id: string) => invoke<void>("prewarm_project", { id }),
  surfaceStatus: (id: string) => invoke<SurfaceStatusDto>("surface_status", { id }),
  openSurfaceUrl: (id: string) => invoke<void>("open_surface_url", { id }),
  contextCard: (id: string) => invoke<ContextCardDto>("context_card", { id }),
  saveContextCard: (id: string, content: string) =>
    invoke<void>("save_context_card", { id, content }),
  errorTail: (id: string) => invoke<ErrorTailDto[]>("error_tail", { id }),
  handOff: (id: string, prompt: string) =>
    invoke<string>("hand_off", { id, prompt }),
  openClaudeTerminal: (id: string, prompt: string) =>
    invoke<void>("open_claude_terminal", { id, prompt }),
  overnightReport: () => invoke<RunRecordDto[]>("overnight_report", {}),
  recentRuns: () => invoke<RunRecordDto[]>("recent_runs", {}),
  openSurfaceWindow: (id: string) => invoke<void>("open_surface_window", { id }),

  /** Disk usage of one project root. Seconds on a cold cache -- see StorageView. */
  projectUsage: (id: string) => invoke<DirUsageDto>("project_usage", { id }),
  /** Immediate children of a directory inside a registered project, largest first. */
  folderChildren: (path: string) => invoke<DirChildDto[]>("folder_children", { path }),
  /** What a project says about itself: README summary, docs, declared scripts. */
  projectManual: (id: string) => invoke<ManualDto>("project_manual", { id }),
  /** Opens a file inside a registered project with its default handler. */
  openPath: (path: string) => invoke<void>("open_path", { path }),
  /** Saves a URL as a launchable project backed by a real .url shortcut. */
  registerWebApp: (name: string, url: string) =>
    invoke<ProjectDto>("register_web_app", { name, url }),
  getSetting: (key: string) => invoke<string | null>("get_setting", { key }),
  setSetting: (key: string, value: string) =>
    invoke<void>("set_setting", { key, value }),
};

// ---- Storage --------------------------------------------------------------------

/** A measured directory tree. Byte counts are summed file lengths. */
export interface DirUsageDto {
  bytes: number;
  files: number;
  /** Entries not read (permissions) or not followed (junctions, symlinks). */
  skipped: number;
}

/** One immediate child of a measured directory. */
export interface DirChildDto {
  name: string;
  path: string;
  isDir: boolean;
  usage: DirUsageDto;
}

// ---- Manual ---------------------------------------------------------------------

/** A markdown doc found at the project root or in `docs/`. */
export interface DocFileDto {
  name: string;
  path: string;
}

/** A named command the project declares in its own manifest. */
export interface ScriptDto {
  name: string;
  command: string;
}

/**
 * What a project says about itself, read off disk -- never generated.
 * A `null` summary means the project has no README prose, which the panel
 * states plainly rather than papering over with an invented description.
 */
export interface ManualDto {
  summary: string | null;
  readme: string | null;
  docs: DocFileDto[];
  scripts: ScriptDto[];
}

// ---- State-machine helpers (mirror RunState::can_start / can_stop) ---------------

export function canStart(state: RunStateDto | undefined): boolean {
  const tag = state?.tag ?? "idle";
  return tag === "idle" || tag === "exited" || tag === "crashed";
}

export function canStop(state: RunStateDto | undefined): boolean {
  const tag = state?.tag ?? "idle";
  return tag === "running" || tag === "starting" || tag === "restarting";
}

export function isTransitional(state: RunStateDto | undefined): boolean {
  const tag = state?.tag ?? "idle";
  return tag === "starting" || tag === "stopping" || tag === "restarting";
}

export function isLive(state: RunStateDto | undefined): boolean {
  const tag = state?.tag ?? "idle";
  return tag === "running" || tag === "stopping";
}

/**
 * A saved web app: a URL registered as a project by the `web` runner.
 *
 * These do not launch a process. "Run" opens them in an embedded window, so
 * every Run call site has to ask this before reaching for `startProject` --
 * spawning `cmd /c start` would record a run that exits instantly and litter
 * the activity list with deaths that never happened.
 */
export function isWebApp(project: ProjectDto): boolean {
  return project.runnerId === "web";
}
