import { listen } from "@tauri-apps/api/event";
import { useQueryClient } from "@tanstack/react-query";
import { FolderPlus, SearchX } from "lucide-react";
import { lazy, Suspense, useEffect, useMemo, useRef, type ReactNode } from "react";

import { Button } from "@/components/ui/Button";
import { Toaster } from "@/components/ui/Toaster";
import { TooltipProvider } from "@/components/ui/Tooltip";
import { LogPanel } from "@/features/logs/LogPanel";
import { PromptStudio } from "@/features/studio/PromptStudio";
import { DetailBar } from "@/features/projects/DetailBar";
import { ProjectGrid } from "@/features/projects/ProjectGrid";
import { ProjectTable } from "@/features/projects/ProjectTable";
import { comparatorFor, sortNeedsMetrics } from "@/features/projects/sort";
import {
  defaultSort,
  useProjects,
} from "@/features/projects/useProjects";
import { searchScore } from "@/features/projects/search";
import { Sidebar } from "@/features/sidebar/Sidebar";
import { TodayStrip } from "@/features/today/TodayStrip";
import { Toolbar } from "@/features/toolbar/Toolbar";
import { api, asIpcError, isLive, type ProjectDto } from "@/lib/ipc";
import { DURATION, usePresentValue } from "@/lib/presence";
import { useRuntime } from "@/state/runtime";
import { useUi, type Collection } from "@/state/ui";

/** Stable identity so the conditional metrics selector cannot cause a loop. */
const NO_METRICS: Record<string, never> = {};

/**
 * Diagnostics and System are code-split: they are screens you open when
 * something is wrong, not part of the daily path, and their chunks should not
 * be parsed on every boot by everyone who never opens them.
 */
const DashboardView = lazy(() =>
  import("@/features/dashboard/DashboardView").then((m) => ({
    default: m.DashboardView,
  })),
);
const StorageView = lazy(() =>
  import("@/features/storage/StorageView").then((m) => ({
    default: m.StorageView,
  })),
);
const ManualPanel = lazy(() =>
  import("@/features/manual/ManualPanel").then((m) => ({
    default: m.ManualPanel,
  })),
);
const AddWebAppDialog = lazy(() =>
  import("@/features/projects/AddWebAppDialog").then((m) => ({
    default: m.AddWebAppDialog,
  })),
);
const AddShortcutDialog = lazy(() =>
  import("@/features/add/AddShortcutDialog").then((m) => ({
    default: m.AddShortcutDialog,
  })),
);
const DiagnosticsView = lazy(() =>
  import("@/features/diagnostics/DiagnosticsView").then((m) => ({
    default: m.DiagnosticsView,
  })),
);


/**
 * The add/scan dialogs are code-split and mounted only while open.
 *
 * They were imported eagerly and rendered unconditionally, so their form
 * inputs, validation and scan flow were parsed on every boot by everyone -- to
 * render nothing at all until the user clicks Add. Nothing paints until the
 * main chunk is parsed, so that sat directly on the critical path.
 *
 * Worth 6.4 KB off the main chunk. Radix Dialog itself stays behind, because
 * `components/ui/Dialog` is shared with the rename flow -- so this moves the
 * dialog-specific code and not the primitive.
 */
const AddProjectDialog = lazy(() =>
  import("@/features/add/AddProjectDialog").then((m) => ({
    default: m.AddProjectDialog,
  })),
);
const ScanDialog = lazy(() =>
  import("@/features/add/ScanDialog").then((m) => ({ default: m.ScanDialog })),
);

function inCollection(
  project: ProjectDto,
  collection: Collection,
  liveIds: ReadonlySet<string>,
): boolean {
  switch (collection.kind) {
    case "all":
      return !project.archived;
    case "favorites":
      return !project.archived && project.favorite;
    case "running":
      return !project.archived && liveIds.has(project.id);
    case "archived":
      return project.archived;
    case "language":
      return !project.archived && project.language === collection.language;
  }
}

export default function App() {
  const { data: projects, isLoading, isError, error, refetch } = useProjects();
  const queryClient = useQueryClient();

  // Report the first painted list to the backend, once.
  //
  // Every other boot figure measures something nobody waits for: the window
  // appears before `setup()` finishes, and timing this over CDP distorts
  // WebView2's own startup. This is the number that matches the wait.
  const reportedReady = useRef(false);
  useEffect(() => {
    if (reportedReady.current || projects === undefined) return;
    reportedReady.current = true;
    // After paint, not during render -- measuring before the browser has put
    // pixels up would report a boot that had not finished.
    requestAnimationFrame(() => {
      // Split the frontend's share so it can be optimised, rather than
      // reported as one opaque number the way `setup` was.
      const nav = performance.getEntriesByType("navigation")[0] as
        | PerformanceNavigationTiming
        | undefined;
      const mark = (name: string) =>
        Math.round(performance.getEntriesByName(name)[0]?.startTime ?? 0);
      void api
        .uiReady({
          domInteractiveMs: Math.round(nav?.domInteractive ?? 0),
          scriptEvalMs: mark("script-eval"),
          reactMountMs: mark("react-mount"),
          fetchStartMs: mark("projects-fetch-start"),
          dataArrivedMs: mark("projects-arrived"),
          paintedMs: Math.round(performance.now()),
        })
        .catch(() => {});
    });
  }, [projects]);
  const query = useUi((s) => s.query);
  const collection = useUi((s) => s.collection);
  const selectedId = useUi((s) => s.selectedId);
  const logsFor = useUi((s) => s.logsFor);
  const studioFor = useUi((s) => s.studioFor);
  const openAddDialog = useUi((s) => s.openAddDialog);
  const states = useRuntime((s) => s.states);

  const liveIds = useMemo(
    () =>
      new Set(
        Object.entries(states)
          .filter(([, s]) => isLive(s))
          .map(([id]) => id),
      ),
    [states],
  );

  const sort = useUi((s) => s.sort);
  const destination = useUi((s) => s.destination);
  // Gate the lazy dialogs so their chunks are fetched only on first use.
  const addDialogOpen = useUi((s) => s.addDialogOpen);
  const scanDialogOpen = useUi((s) => s.scanDialogOpen);
  const webDialogOpen = useUi((s) => s.webDialogOpen);
  const shortcutDialogOpen = useUi((s) => s.shortcutDialogOpen);
  const view = useUi((s) => s.view);
  const manualOpen = useUi((s) => s.manualOpen);

  // Metrics tick every second. Subscribing unconditionally would re-render this
  // whole tree once a second forever; the CPU column is the only sort that
  // actually needs them, so everything else keeps a stable empty object and
  // never wakes up.
  const metrics = useRuntime((s) => (sortNeedsMetrics(sort) ? s.metrics : NO_METRICS));

  const visible = useMemo(() => {
    const all = projects ?? [];
    const inScope = all.filter((p) => inCollection(p, collection, liveIds));
    // While a query is live, relevance IS the order: the whole point of
    // ranked search is that the best match sits on top for Enter to take.
    // Column sorts resume the moment the query clears.
    const needle = query.trim();
    if (needle.length > 0) {
      return inScope
        .map((p) => ({ p, score: searchScore(p, needle) }))
        .filter((e): e is { p: (typeof inScope)[number]; score: number } => e.score !== null)
        .sort((a, b) => b.score - a.score)
        .map((e) => e.p);
    }
    // `.sort` mutates, and `filter` already produced a fresh array, so this is
    // safe -- but never sort `projects` itself, which React Query owns.
    return sort
      ? inScope.sort(comparatorFor(sort, { states, metrics }))
      : inScope.sort(defaultSort);
  }, [projects, query, collection, liveIds, sort, states, metrics]);

  // Files dropped anywhere on the window feed the add flow.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void listen<{ paths: string[] }>("tauri://drag-drop", (event) => {
      const first = event.payload.paths[0];
      if (first) useUi.getState().openAddDialog(first);
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // The startup rescan runs after the window is up, so anything it finds
  // arrives once the list has already painted.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void listen<number>("deck://projects", (event) => {
      void queryClient.invalidateQueries({ queryKey: ["projects"] });
      // The scan announces its finds -- silently growing the list reads as
      // "was that always there?" a week later.
      const n = event.payload;
      if (typeof n === "number" && n > 0) {
        useUi
          .getState()
          .toast("info", `Scan found ${n} new project${n === 1 ? "" : "s"}`);
      }
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [queryClient]);

  const selectedProject = useMemo(
    () => (projects ?? []).find((p) => p.id === selectedId) ?? null,
    [projects, selectedId],
  );
  const logProject = useMemo(
    () => (projects ?? []).find((p) => p.id === logsFor) ?? null,
    [projects, logsFor],
  );
  const studioProject = useMemo(
    () => (projects ?? []).find((p) => p.id === studioFor) ?? null,
    [projects, studioFor],
  );

  // Both panels animate out as well as in -- vanishing instantly reads as a
  // rendering glitch rather than a dismissal. These hold the departing project
  // through the exit, because the id is already cleared by then and the panel
  // would otherwise animate out as an empty shell. Gated on the destination so
  // a project's controls never sit under a screen that is not about it.
  const detail = usePresentValue(
    destination === "projects" ? selectedProject : null,
    DURATION.fast,
  );
  const logs = usePresentValue(
    destination === "projects" ? logProject : null,
    DURATION.base,
  );
  const studio = usePresentValue(
    destination === "projects" ? studioProject : null,
    DURATION.base,
  );

  return (
    <TooltipProvider delayDuration={350}>
      <div className="flex h-full flex-col">
        <Toolbar projectCount={projects?.length ?? 0} />

        <div className="deck-split flex min-h-0 flex-1">
          <Sidebar projects={projects ?? []} />

          <div className="deck-main flex min-w-0 flex-1 flex-col bg-field">
            {destination === "projects" && !isLoading && !isError && (
              <TodayStrip projects={projects ?? []} />
            )}
            <main
              className="@container min-h-0 flex-1 overflow-y-auto p-4"
              onClick={(e) => {
                // Clicking empty canvas clears the selection.
                if (e.target === e.currentTarget) useUi.getState().select(null);
              }}
            >
              {destination === "dashboard" && (
                <Suspense
                  fallback={
                    <div
                      role="status"
                      className="mx-auto flex w-full max-w-[1100px] flex-col gap-3"
                      aria-label="Loading dashboard"
                    >
                      <div className="h-8 w-40 animate-pulse rounded-control bg-panel" />
                      <div className="grid grid-cols-2 gap-3 @min-[640px]:grid-cols-4">
                        {Array.from({ length: 4 }, (_, i) => (
                          <div key={i} className="h-[104px] animate-pulse rounded-card bg-panel" />
                        ))}
                      </div>
                      <div className="h-40 animate-pulse rounded-card bg-panel" />
                    </div>
                  }
                >
                  <DashboardView projects={projects ?? []} />
                </Suspense>
              )}

              {destination === "storage" && (
                <Suspense
                  fallback={
                    <div
                      role="status"
                      aria-label="Loading storage"
                      className="mx-auto flex w-full max-w-[1180px] flex-col gap-3"
                    >
                      <div className="h-9 w-32 animate-pulse rounded-control bg-panel" />
                      <div className="h-64 animate-pulse rounded-card bg-panel" />
                    </div>
                  }
                >
                  <StorageView projects={projects ?? []} />
                </Suspense>
              )}

              {destination === "diagnostics" && (
                <Suspense
                  fallback={
                    <div role="status" className="flex flex-col gap-2" aria-label="Loading diagnostics">
                      {Array.from({ length: 6 }, (_, i) => (
                        <div key={i} className="h-12 animate-pulse rounded-panel bg-panel" />
                      ))}
                    </div>
                  }
                >
                  <DiagnosticsView />
                </Suspense>
              )}

              {destination === "projects" && isLoading && <ListSkeleton />}

              {destination === "projects" && isError && (
                <EmptyState
                  icon={<SearchX size={26} className="text-signal/70" />}
                  title="Could not load projects"
                  // `asIpcError`, never `instanceof Error`: a rejected Tauri
                  // command yields a plain `{code, message}` object, so an
                  // instanceof check throws away the only useful detail and
                  // leaves a failure with no evidence.
                  body={asIpcError(error).message}
                  action={
                    <Button variant="outline" onClick={() => void refetch()}>
                      Try again
                    </Button>
                  }
                />
              )}

              {destination === "projects" && !isLoading && !isError && visible.length === 0 && (
                query.trim().length > 0 || collection.kind !== "all" ? (
                  <EmptyState
                    icon={<SearchX size={26} className="text-ink/30" />}
                    title="Nothing here"
                    body={
                      query.trim().length > 0
                        ? `No matches for "${query.trim()}" — names, languages, tags and paths are all searched.`
                        : "This collection is empty."
                    }
                  />
                ) : (
                  <EmptyState
                    icon={<FolderPlus size={26} className="text-ink/30" />}
                    title="No projects yet"
                    body="Add a project folder, scan a workspace, or drop a folder anywhere on this window."
                    action={
                      <Button variant="primary" onClick={() => openAddDialog()}>
                        Add your first project
                      </Button>
                    }
                  />
                )
              )}

              {destination === "projects" && !isLoading && !isError && visible.length > 0 &&
                (view === "grid" ? (
                  <ProjectGrid projects={visible} />
                ) : (
                  <ProjectTable projects={visible} />
                ))}
            </main>

            {detail.item && (
              <DetailBar key={detail.item.id} project={detail.item} open={detail.open} />
            )}
          </div>

          {/* The manual is a reference pane, not a project panel: it stays put
              while the selection changes underneath it, so it is mounted on
              the split rather than keyed to a project.

              Only where a project CAN be selected, though. On Dashboard and
              Diagnostics it held 340px to display one sentence saying it had
              nothing to show -- the DetailBar is suppressed on those same
              screens for exactly this reason (see `detail` above). Storage
              counts, because its rows select the project they measure. */}
          {/* ...and never beside the log or prompt panels. All three are side
              panes, and at the app's common 1240px width the sidebar plus two
              of them crushed the library column to ZERO -- measured by
              layout-verify as a 0px name column. They also answer different
              moments: the manual is "what is this", the log is "what just
              happened". Wanting both at once is not a real state. */}
          {manualOpen &&
            !logs.item &&
            !studio.item &&
            (destination === "projects" || destination === "storage") && (
              <Suspense fallback={null}>
                <ManualPanel project={selectedProject} />
              </Suspense>
            )}

          {logs.item && (
            <LogPanel key={logs.item.id} project={logs.item} open={logs.open} />
          )}
          {studio.item && (
            <PromptStudio key={studio.item.id} project={studio.item} open={studio.open} />
          )}
        </div>
      </div>

      {/* Mounted only once opened. No fallback: a dialog nobody asked for
          should render nothing, not a spinner. */}
      {addDialogOpen && (
        <Suspense fallback={null}>
          <AddProjectDialog />
        </Suspense>
      )}

      {webDialogOpen && (
        <Suspense fallback={null}>
          <AddWebAppDialog />
        </Suspense>
      )}

      {shortcutDialogOpen && (
        <Suspense fallback={null}>
          <AddShortcutDialog />
        </Suspense>
      )}
      {scanDialogOpen && (
        <Suspense fallback={null}>
          <ScanDialog />
        </Suspense>
      )}
      <Toaster />
    </TooltipProvider>
  );
}

function EmptyState({
  icon,
  title,
  body,
  action,
}: {
  icon: ReactNode;
  title: string;
  body: string;
  action?: ReactNode;
}) {
  return (
    <div className="flex h-full min-h-64 flex-col items-center justify-center gap-3 text-center">
      {icon}
      <div>
        <h2 className="text-sm font-semibold text-ink">{title}</h2>
        <p className="mx-auto mt-1 max-w-sm text-xs leading-relaxed text-ink/50">{body}</p>
      </div>
      {action}
    </div>
  );
}

/** Loading placeholder shaped like the tile grid it precedes. */
/**
 * Loading placeholder shaped like whatever is ACTUALLY about to render.
 *
 * It used to draw cards unconditionally, so opening the app in table view
 * showed a grid of tiles and then replaced them with rows -- the layout visibly
 * changed shape as data landed, which reads as a glitch rather than as loading.
 * A skeleton that is not the shape of its content is worse than no skeleton: it
 * makes a promise the render then breaks.
 */
function ListSkeleton() {
  // Table: same 48px rhythm and the same light rule between rows, so the real
  // list lands exactly where the placeholder was.
  return (
    <div className="overflow-hidden rounded-panel" aria-hidden>
      <div className="h-[33px] border-b border-rule bg-field" />
      {Array.from({ length: 12 }, (_, i) => (
        <div
          key={i}
          className="flex h-12 animate-pulse items-center gap-2.5 border-b border-rule/70 bg-field px-3 last:border-b-0"
        >
          <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-raised" />
          <span className="ml-6 h-6 w-6 shrink-0 rounded-[5px] bg-raised" />
          <span className="h-2 w-40 rounded bg-raised" />
          <span className="ml-auto h-2 w-16 rounded bg-raised" />
        </div>
      ))}
    </div>
  );
}
